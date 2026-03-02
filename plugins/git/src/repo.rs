//! Git repository wrapper — HEAD blob, diff, index ops, branch/tag listing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::from_utf8;

use color_eyre::eyre::{Result, WrapErr};
use nyne::types::vfs_path::VfsPath;
use parking_lot::Mutex;
use tracing::debug;

use crate::commit::diff_opts;

/// Shared git repository handle.
///
/// Wraps `git2::Repository` in a `Mutex` (`Repository` is `Send` but not `Sync`).
/// Computes and caches the path prefix to convert `VfsPath` → git-relative path.
pub struct GitRepo {
    repo: Mutex<git2::Repository>,
    /// Prefix to prepend to `VfsPath` for git-relative paths.
    /// Empty when source dir == workdir.
    prefix: String,
}

impl GitRepo {
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
            repo: Mutex::new(repo),
            prefix,
        })
    }

    /// Convert a `VfsPath` to a git-relative path.
    pub fn rel_path(&self, path: &VfsPath) -> String {
        if self.prefix.is_empty() {
            path.as_str().to_owned()
        } else if path.is_root() {
            self.prefix.clone()
        } else {
            format!("{}/{}", self.prefix, path.as_str())
        }
    }

    /// The path to the `.git` directory (or equivalent for worktrees).
    ///
    /// Returns the value of `git2::Repository::path()`, which is the
    /// absolute path to the git directory (e.g., `/project/.git/`).
    pub(crate) fn git_dir_path(&self) -> PathBuf { self.lock().path().to_owned() }

    pub(crate) fn lock(&self) -> parking_lot::MutexGuard<'_, git2::Repository> { self.repo.lock() }

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
            Err(_) => Ok(Vec::new()),
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
    /// the correct blob OID, mtime, and size. Safe during FUSE callbacks
    /// because the repo's stored workdir points to the overlay merged path,
    /// so all stats resolve against the overlay filesystem.
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

    /// Current branch name, or a fallback for detached/unborn states.
    pub fn head_branch(&self) -> String {
        let repo = self.lock();
        match repo.head() {
            Ok(head) => head.shorthand().unwrap_or("HEAD").to_owned(),
            Err(_) => "(no commits)".to_owned(),
        }
    }

    /// HEAD commit timestamp as seconds since epoch, or 0 for unborn branches.
    pub(crate) fn head_epoch_secs(&self) -> i64 {
        let repo = self.lock();
        repo.head()
            .ok()
            .and_then(|h| h.peel_to_commit().ok())
            .map_or(0, |c| c.time().seconds())
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
    pub fn extension_counts(&self) -> Vec<(String, usize)> {
        let Ok(paths) = self.index_paths() else {
            return vec![];
        };
        let mut counts: HashMap<String, usize> = HashMap::new();
        for path in &paths {
            if let Some(ext) = Path::new(path.as_str()).extension().and_then(|e| e.to_str()) {
                *counts.entry(ext.to_owned()).or_default() += 1;
            }
        }
        let mut sorted: Vec<_> = counts.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        sorted
    }
}

/// Resolve HEAD to a tree, returning `None` for unborn branches.
fn head_tree(repo: &git2::Repository) -> Result<Option<git2::Tree<'_>>> {
    match repo.head() {
        Ok(head) => {
            let tree = head
                .peel(git2::ObjectType::Tree)?
                .into_tree()
                .map_err(|_| color_eyre::eyre::eyre!("HEAD does not resolve to a tree"))?;
            Ok(Some(tree))
        }
        Err(e) if e.code() == git2::ErrorCode::UnbornBranch => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Format a `git2::Diff` as a unified diff string.
fn format_diff(diff: &git2::Diff<'_>) -> Result<String> {
    let mut output = String::new();
    diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        let origin = line.origin();
        // Content lines get their origin character as prefix
        if matches!(origin, '+' | '-' | ' ') {
            output.push(origin);
        }
        // File/hunk headers are printed as-is from content
        if let Ok(content) = from_utf8(line.content()) {
            output.push_str(content);
        }
        true
    })?;
    Ok(output)
}
