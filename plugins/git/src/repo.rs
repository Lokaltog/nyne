//! Git repository wrapper — HEAD blob, diff, index ops, branch/tag listing.
//!
//! [`Repo`] wraps a `git2::Repository` behind a `Mutex` and provides
//! high-level methods for all git operations needed by the VFS provider.
//! The repository is opened once during plugin activation using pre-mount
//! real paths and shared via `Arc<Repo>` across all providers.
//!
//! **Threading:** All methods acquire the mutex internally via [`lock()`],
//! so callers can use `&self` without external synchronization.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::str::from_utf8;

use color_eyre::eyre::{WrapErr, eyre};
use nyne::prelude::*;
use parking_lot::Mutex;
use tracing::{debug, warn};

use crate::commit::diff_opts;

/// Shared git repository handle.
///
/// Wraps `git2::Repository` in a `Mutex` (`Repository` is `Send` but not `Sync`).
/// Computes and caches the path prefix to convert absolute paths → git-relative paths.
pub struct Repo {
    inner: Mutex<git2::Repository>,
    /// Prefix to prepend for git-relative paths.
    /// Empty when source dir == workdir.
    prefix: String,
    /// Project root directory — used to convert absolute paths to repo-relative.
    root: PathBuf,
}

/// Core git operations — open, blob retrieval, diff, index, branches, and tags.
impl Repo {
    /// Discover and open the git repository containing `source_dir`.
    pub(crate) fn open(source_dir: &Path) -> Result<Self> {
        let repo = git2::Repository::discover(source_dir)
            .wrap_err_with(|| format!("no git repository at {}", source_dir.display()))?;

        let workdir = repo
            .workdir()
            .ok_or_else(|| color_eyre::eyre::eyre!("bare repositories are not supported"))?;

        let prefix = source_dir
            .strip_prefix(workdir)
            .unwrap_or_else(|_| Path::new(""))
            .to_string_lossy()
            .into_owned();

        debug!(
            workdir = %workdir.display(),
            prefix = %prefix,
            "opened git repository"
        );

        Ok(Self {
            inner: Mutex::new(repo),
            prefix,
            root: source_dir.to_owned(),
        })
    }

    /// Convert an absolute source path to a git-relative path.
    pub fn rel_path(&self, path: &Path) -> String {
        let relative = path.strip_prefix(&self.root).unwrap_or(path).to_string_lossy();
        if self.prefix.is_empty() {
            relative.into_owned()
        } else if relative.is_empty() {
            self.prefix.clone()
        } else {
            format!("{}/{relative}", self.prefix)
        }
    }

    /// Acquires a lock on the underlying git2 repository.
    pub(crate) fn lock(&self) -> parking_lot::MutexGuard<'_, git2::Repository> { self.inner.lock() }

    /// Retrieve file content at HEAD from the object store.
    ///
    /// Returns empty `Vec` if the file doesn't exist at HEAD (new file)
    /// or the branch is unborn.
    pub(crate) fn head_blob(&self, rel_path: &str) -> Result<Vec<u8>> {
        let repo = self.lock();
        let Some(tree) = head_tree(&repo)? else {
            return Ok(Vec::new());
        };
        match tree.get_path(Path::new(rel_path)) {
            Ok(entry) => {
                let blob = repo.find_blob(entry.id())?;
                Ok(blob.content().to_vec())
            }
            Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(Vec::new()),
            Err(e) => Err(e.into()),
        }
    }

    /// Resolve a ref (branch name, tag, sha) to its commit tree.
    fn ref_tree<'r>(repo: &'r git2::Repository, ref_name: &str) -> Result<git2::Tree<'r>> {
        let obj = repo
            .revparse_single(ref_name)
            .wrap_err_with(|| format!("cannot resolve ref '{ref_name}'"))?;
        obj.peel(git2::ObjectType::Tree)?
            .into_tree()
            .map_err(|_| color_eyre::eyre::eyre!("'{ref_name}' does not resolve to a tree"))
    }

    /// List entries in a directory at an arbitrary ref.
    ///
    /// `tree_path` is the directory path within the tree (empty string for root).
    /// Returns `(name, is_directory)` pairs sorted by name.
    pub(crate) fn ref_tree_entries(&self, ref_name: &str, tree_path: &str) -> Result<Vec<(String, bool)>> {
        let repo = self.lock();
        let root = Self::ref_tree(&repo, ref_name)?;
        let tree = if tree_path.is_empty() {
            root
        } else {
            let entry = root.get_path(Path::new(tree_path))?;
            repo.find_tree(entry.id())
                .wrap_err_with(|| format!("'{tree_path}' is not a directory in '{ref_name}'"))?
        };
        let mut entries: Vec<(String, bool)> = tree
            .iter()
            .filter_map(|e| {
                let name = e.name()?.to_owned();
                let is_dir = e.kind() == Some(git2::ObjectType::Tree);
                Some((name, is_dir))
            })
            .collect();
        entries.sort_by(|(a, _), (b, _)| a.cmp(b));
        Ok(entries)
    }

    /// Read a blob (file content) at an arbitrary ref and path.
    pub(crate) fn blob_at_ref(&self, ref_name: &str, file_path: &str) -> Result<Vec<u8>> {
        let repo = self.lock();
        let tree = Self::ref_tree(&repo, ref_name)?;
        let entry = tree.get_path(Path::new(file_path))?;
        let blob = repo.find_blob(entry.id())?;
        Ok(blob.content().to_vec())
    }

    /// Diff of HEAD against an arbitrary ref (branch, tag, sha) for a single file.
    pub(crate) fn diff_ref(&self, rel_path: &str, refspec: &str) -> Result<String> {
        let repo = self.lock();
        let their_tree = Self::ref_tree(&repo, refspec)?;
        let our_tree = head_tree(&repo)?;
        let mut opts = diff_opts(rel_path);
        let diff = repo.diff_tree_to_tree(Some(&their_tree), our_tree.as_ref(), Some(&mut opts))?;
        format_diff(&diff)
    }

    /// Check whether a file is tracked in the git index.
    pub(crate) fn is_tracked(&self, rel_path: &str) -> Result<bool> {
        let repo = self.lock();
        let index = repo.index().wrap_err("failed to open git index")?;
        Ok(index.get_path(Path::new(rel_path), 0).is_some())
    }

    /// Update the git index after a file rename on the real filesystem.
    ///
    /// Mirrors porcelain `git mv` (which is just `mv` + `git rm` + `git add`).
    /// Uses `add_path` which re-stats the file at the new path to record
    /// the correct blob OID, mtime, and size.
    pub(crate) fn index_rename_with_stat(&self, old_rel: &str, new_rel: &str) -> Result<()> {
        let repo = self.lock();
        let mut index = repo.index().wrap_err("failed to open git index")?;
        index.remove_path(Path::new(old_rel))?;
        index.add_path(Path::new(new_rel))?;
        index.write().wrap_err("failed to write git index")?;
        debug!(old = old_rel, new = new_rel, "git index rename (with stat)");
        Ok(())
    }

    /// List local branch names.
    pub(crate) fn branches(&self) -> Result<Vec<String>> {
        let repo = self.lock();
        let mut names = Vec::new();
        for branch_result in repo.branches(Some(git2::BranchType::Local))? {
            let (branch, _): (git2::Branch<'_>, _) = branch_result?;
            if let Some(name) = branch.name()? {
                names.push(name.to_owned());
            }
        }
        names.sort();
        Ok(names)
    }

    /// List tag names.
    pub(crate) fn tags(&self) -> Result<Vec<String>> {
        let repo = self.lock();
        let tag_names = repo.tag_names(None)?;
        let mut names: Vec<String> = tag_names.iter().flatten().map(String::from).collect();
        names.sort();
        Ok(names)
    }

    /// Rename a local branch.
    pub(crate) fn rename_branch(&self, old_name: &str, new_name: &str) -> Result<()> {
        let repo = self.lock();
        let mut branch = repo
            .find_branch(old_name, git2::BranchType::Local)
            .wrap_err_with(|| format!("branch not found: {old_name}"))?;
        branch
            .rename(new_name, false)
            .wrap_err_with(|| format!("failed to rename branch {old_name} to {new_name}"))?;
        debug!(old = old_name, new = new_name, "branch renamed");
        Ok(())
    }

    /// Delete a local branch, only if it is fully merged into HEAD.
    ///
    /// Refuses to delete the current HEAD branch or any branch whose tip
    /// commit is not an ancestor of HEAD.
    pub(crate) fn delete_branch(&self, name: &str) -> Result<()> {
        let repo = self.lock();
        let mut branch = repo
            .find_branch(name, git2::BranchType::Local)
            .wrap_err_with(|| format!("branch not found: {name}"))?;

        if branch.is_head() {
            return Err(io_err(
                io::ErrorKind::PermissionDenied,
                format!("refusing to delete the current branch: {name}"),
            ));
        }

        let branch_oid = branch
            .get()
            .target()
            .ok_or_else(|| eyre!("branch {name} has no target commit"))?;
        let head_oid = repo
            .head()
            .wrap_err("failed to resolve HEAD")?
            .target()
            .ok_or_else(|| eyre!("HEAD has no target commit"))?;

        // graph_descendant_of returns false when OIDs are equal, but a
        // branch at the same commit as HEAD is trivially merged.
        let merged = branch_oid == head_oid
            || repo
                .graph_descendant_of(head_oid, branch_oid)
                .wrap_err("merge-base check failed")?;
        if !merged {
            return Err(io_err(
                io::ErrorKind::PermissionDenied,
                format!("branch {name} is not fully merged into HEAD"),
            ));
        }

        branch
            .delete()
            .wrap_err_with(|| format!("failed to delete branch {name}"))?;
        debug!(branch = name, "branch deleted (merged)");
        Ok(())
    }

    /// Current branch name, or a fallback for detached/unborn states.
    pub fn head_branch(&self) -> String {
        let repo = self.lock();
        match repo.head() {
            Ok(head) => head.shorthand().unwrap_or("HEAD").to_owned(),
            Err(_) => "(no commits)".to_owned(),
        }
    }

    /// All file paths in the git index.
    ///
    /// Returns paths relative to the repository workdir. Useful for
    /// language detection, file enumeration, and other index-level queries.
    pub fn index_paths(&self) -> Result<Vec<String>> {
        let repo = self.lock();
        let index = repo.index().wrap_err("failed to read git index")?;
        let paths = index
            .iter()
            .filter_map(|entry| from_utf8(&entry.path).ok().map(String::from))
            .collect();
        Ok(paths)
    }

    /// Count file extensions in the git index, sorted by frequency (descending).
    ///
    /// Counts ALL extensions — not filtered by any registry. Consumers who
    /// want a subset can filter the result themselves.
    pub fn extension_counts(&self) -> Result<Vec<(String, usize)>> {
        let repo = self.lock();
        let index = repo.index().wrap_err("failed to read git index")?;
        let mut counts: HashMap<String, usize> = HashMap::new();
        for entry in index.iter() {
            let Ok(path) = from_utf8(&entry.path) else { continue };
            if let Some(ext) = Path::new(path).extension_str() {
                *counts.entry(ext.to_owned()).or_default() += 1;
            }
        }
        let mut sorted: Vec<_> = counts.into_iter().collect();
        sorted.sort_by_key(|x| Reverse(x.1));
        Ok(sorted)
    }
}

/// Resolve HEAD to a reference, returning `None` for unborn branches.
fn resolve_head(repo: &git2::Repository) -> Result<Option<git2::Reference<'_>>> {
    match repo.head() {
        Ok(head) => Ok(Some(head)),
        Err(e) if e.code() == git2::ErrorCode::UnbornBranch => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Resolve HEAD to a tree, returning `None` for unborn branches.
fn head_tree(repo: &git2::Repository) -> Result<Option<git2::Tree<'_>>> {
    let Some(head) = resolve_head(repo)? else {
        return Ok(None);
    };
    Ok(Some(
        head.peel(git2::ObjectType::Tree)?
            .into_tree()
            .map_err(|_| eyre!("HEAD does not resolve to a tree"))?,
    ))
}

/// Format a `git2::Diff` as a unified diff string.
///
/// Iterates all patch lines, prefixing content lines with their origin
/// character (`+`, `-`, ` `) while passing file and hunk headers through
/// verbatim.
fn format_diff(diff: &git2::Diff<'_>) -> Result<String> {
    let mut output = String::new();
    diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        let origin = line.origin();
        // Content lines get their origin character as prefix
        if matches!(origin, '+' | '-' | ' ') {
            output.push(origin);
        }
        // File/hunk headers are printed as-is from content
        let content = line.content();
        if let Ok(s) = str::from_utf8(content) {
            output.push_str(s);
        } else {
            warn!("lossy UTF-8 conversion in diff line (origin={origin:?})");
            output.push_str(&String::from_utf8_lossy(content));
        }
        true
    })?;
    Ok(output)
}
