//! Branch decomposition — slash-separated branch names as nested directories.
//!
//! Git branch names like `feat/lsp-diag-fix` are decomposed into nested
//! directory trees (`branches/feat/lsp-diag-fix/`) so that each path segment
//! is a valid FUSE directory entry (no `/` in names).

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use color_eyre::eyre::{Result, eyre};
use nyne::router::{
    AffectedFiles, Filesystem, NamedNode, Node, ReadContext, Readable, RenameContext, Renameable, UnlinkContext,
    Unlinkable,
};

use crate::repo::Repo;

/// Renameable capability for branch directory nodes.
///
/// When the user renames a branch directory (e.g., `mv branches/old branches/new`),
/// this performs the actual `git branch -m` via libgit2.
///
/// For slashed branches, only the leaf segment is renamed — the prefix is preserved.
/// Renaming `branches/feat/old` to `new` produces git branch `feat/new`.
pub(super) struct BranchRename {
    pub repo: Arc<Repo>,
    pub branch_name: String,
}

/// [`Renameable`] implementation for [`BranchRename`].
impl Renameable for BranchRename {
    /// Renames a git branch, preserving namespace prefix.
    fn rename(&self, ctx: &RenameContext<'_>) -> Result<AffectedFiles> {
        let new_segment = rename_target_name(ctx)?;
        let new_branch = match self.branch_name.rsplit_once('/') {
            Some((prefix, _)) => format!("{prefix}/{new_segment}"),
            None => new_segment.to_owned(),
        };
        self.repo.rename_branch(&self.branch_name, &new_branch)?;
        Ok(vec![])
    }
}
/// Unlinkable capability for branch directory nodes.
///
/// When the user removes a branch directory (e.g., `rmdir branches/old`),
/// this deletes the branch via libgit2 — but only if the branch is fully
/// merged into HEAD.
pub(super) struct BranchRemove {
    pub repo: Arc<Repo>,
    pub branch_name: String,
}

/// [`Unlinkable`] implementation for [`BranchRemove`].
impl Unlinkable for BranchRemove {
    /// Deletes a git branch.
    fn unlink(&self, _ctx: &UnlinkContext<'_>) -> Result<AffectedFiles> {
        self.repo.delete_branch(&self.branch_name)?;
        Ok(vec![])
    }
}

/// Compute the child nodes for a given branch namespace prefix.
///
/// `prefix` is either empty (root level) or ends with `/` (e.g., `"feat/"`).
/// Returns one directory node per unique next-level segment:
/// - **Leaf** segments (prefix + segment = complete branch name) get [`BranchRename`]
///   and [`BranchRemove`] (rmdir deletes merged branches).
/// - **Intermediate** segments (prefix + segment is only a prefix of deeper branches)
///   are plain directories.
///
/// If a name is both a leaf and an intermediate (branch `feat` coexisting with
/// `feat/foo`), it is emitted as a leaf with `BranchRename`.
pub(super) fn branch_segments_at_prefix(repo: &Arc<Repo>, prefix: &str) -> Result<Option<Vec<NamedNode>>> {
    let branches = repo.branches()?;

    let mut segments: BTreeSet<&str> = BTreeSet::new();
    let mut leaf_branches: Vec<(&str, &str)> = Vec::new(); // (segment, full_name)

    for name in &branches {
        let Some(tail) = name.strip_prefix(prefix) else {
            continue;
        };
        if tail.is_empty() {
            continue;
        }
        let segment = tail.split_once('/').map_or(tail, |(s, _)| s);
        segments.insert(segment);

        // Leaf: no further `/` after the segment.
        if !tail[segment.len()..].contains('/') {
            leaf_branches.push((segment, name));
        }
    }

    if segments.is_empty() {
        return Ok(None);
    }

    Ok(Some(
        segments
            .into_iter()
            .map(|segment| {
                let Some((_, full_name)) = leaf_branches.iter().find(|(s, _)| *s == segment) else {
                    return NamedNode::dir(segment);
                };
                let branch_name = (*full_name).to_owned();
                Node::dir()
                    .with_renameable(BranchRename {
                        repo: Arc::clone(repo),
                        branch_name: branch_name.clone(),
                    })
                    .with_unlinkable(BranchRemove {
                        repo: Arc::clone(repo),
                        branch_name,
                    })
                    .named(segment)
            })
            .collect(),
    ))
}

/// File content from a branch's tree — reads the blob at `path` on `branch`.
pub(super) struct BranchBlobContent {
    pub repo: Arc<Repo>,
    pub branch: String,
    pub path: String,
}

/// [`Readable`] implementation for [`BranchBlobContent`].
impl Readable for BranchBlobContent {
    /// Reads a blob from a branch at the given path.
    fn read(&self, _ctx: &ReadContext<'_>) -> Result<Vec<u8>> { self.repo.blob_at_ref(&self.branch, &self.path) }
}

/// Build virtual nodes for the tree entries at `tree_path` on `branch`.
///
/// Directories become `NamedNode::dir`, files become readable nodes
/// backed by [`BranchBlobContent`].
pub(super) fn branch_tree_nodes(repo: &Arc<Repo>, branch: &str, tree_path: &str) -> Result<Option<Vec<NamedNode>>> {
    let entries = repo.ref_tree_entries(branch, tree_path)?;
    Ok(Some(
        entries
            .into_iter()
            .map(|(name, is_dir)| {
                if is_dir {
                    return NamedNode::dir(&name);
                }
                let path = if tree_path.is_empty() {
                    name.clone()
                } else {
                    format!("{tree_path}/{name}")
                };
                Node::file()
                    .with_readable(BranchBlobContent {
                        repo: Arc::clone(repo),
                        branch: branch.to_owned(),
                        path,
                    })
                    .named(&name)
            })
            .collect(),
    ))
}

/// Extract the filename segment of a rename target path.
fn rename_target_name<'a>(ctx: &'a RenameContext<'_>) -> Result<&'a str> {
    ctx.target.file_name().and_then(|n| n.to_str()).ok_or_else(|| eyre!("rename target has no filename"))
}

/// Git-aware file rename: filesystem rename + git index update.
///
/// Attached to companion directory nodes so that `mv old.rs@ new.rs@`
/// performs both the real filesystem rename and a `git mv` equivalent.
pub struct GitFileRename {
    pub(crate) repo: Option<Arc<Repo>>,
    pub(crate) fs: Arc<dyn Filesystem>,
    pub(crate) source_file: PathBuf,
}

/// [`Renameable`] implementation for [`GitFileRename`].
impl Renameable for GitFileRename {
    fn rename(&self, ctx: &RenameContext<'_>) -> Result<AffectedFiles> {
        let new_path = self.source_file.with_file_name(rename_target_name(ctx)?);
        self.fs.rename(&self.source_file, &new_path)?;

        if let Some(repo) = &self.repo {
            let old_rel = repo.rel_path(&self.source_file);
            if repo.is_tracked(&old_rel)? {
                let new_rel = repo.rel_path(&new_path);
                repo.index_rename_with_stat(&old_rel, &new_rel)?;
            }
        }

        Ok(vec![self.source_file.clone(), new_path])
    }
}
