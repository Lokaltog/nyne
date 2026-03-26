//! File view context — repo + path pair for template views.

use std::sync::Arc;

use crate::repo::GitRepo;

/// Context for a git view — repo handle and the git-relative path being inspected.
///
/// Shared by all per-file git view structs (blame, log, contributors, notes)
/// via the [`git_template_view!`](super::git_template_view) macro.
#[derive(Clone)]
pub struct FileViewCtx {
    pub repo: Arc<GitRepo>,
    pub rel_path: String,
}

/// Constructor for [`FileViewCtx`].
impl FileViewCtx {
    /// Creates a new file view context for the given repository path.
    pub fn new(repo: &Arc<GitRepo>, rel_path: String) -> Self {
        Self {
            repo: Arc::clone(repo),
            rel_path,
        }
    }
}
