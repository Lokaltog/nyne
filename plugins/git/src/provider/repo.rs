//! File view context — repo + path pair for template views.

use std::sync::Arc;

use crate::repo::GitRepo;

/// Context for a git view — repo and the relative path being inspected.
#[derive(Clone)]
pub struct FileViewCtx {
    pub repo: Arc<GitRepo>,
    pub rel_path: String,
}

impl FileViewCtx {
    pub fn new(repo: &Arc<GitRepo>, rel_path: String) -> Self {
        Self {
            repo: Arc::clone(repo),
            rel_path,
        }
    }
}
