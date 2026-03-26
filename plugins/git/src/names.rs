//! VFS name constants for git-provided virtual paths.

use nyne::templates::HandleBuilder;

/// Well-known directory name for git content (`@/git/`).
pub(crate) const DIR_GIT: &str = "git";

/// Directory name for branch listing (`@/git/branches/`).
pub(crate) const DIR_BRANCHES: &str = "branches";

/// Directory name for tag listing (`@/git/tags/`).
pub(crate) const DIR_TAGS: &str = "tags";

/// Directory name for file history versions (`file.rs@/history/`).
pub const DIR_HISTORY: &str = "history";

/// Directory name for diff variants (`file.rs@/diff/`).
pub(crate) const DIR_DIFF: &str = "diff";

/// Virtual file for per-file git blame.
pub const FILE_BLAME: &str = "BLAME.md";

/// Virtual file for per-file git log.
pub const FILE_LOG: &str = "LOG.md";

/// Virtual file for per-file git contributors.
pub(crate) const FILE_CONTRIBUTORS: &str = "CONTRIBUTORS.md";

/// Virtual file for per-file git notes.
pub(crate) const FILE_NOTES: &str = "NOTES.md";

/// Virtual file for repository-wide git status.
pub(crate) const FILE_GIT_STATUS: &str = "STATUS.md";

/// Virtual file for HEAD working-directory diff.
pub(crate) const FILE_HEAD_DIFF: &str = "HEAD.diff";

/// Create a [`HandleBuilder`](nyne::templates::HandleBuilder) with git name
/// globals pre-registered for template rendering.
///
/// Chains from [`nyne::handle_builder`] (which registers core globals),
/// then adds git-specific name constants.
pub fn handle_builder() -> HandleBuilder {
    let mut b = nyne::handle_builder();
    nyne::register_globals!(
        b.engine_mut(),
        DIR_GIT,
        DIR_BRANCHES,
        DIR_TAGS,
        DIR_HISTORY,
        DIR_DIFF,
        FILE_BLAME,
        FILE_LOG,
        FILE_CONTRIBUTORS,
        FILE_NOTES,
        FILE_GIT_STATUS,
        FILE_HEAD_DIFF,
    );
    b
}
