//! VFS name constants for git-provided virtual paths.

use nyne::templates::HandleBuilder;

/// Well-known directory name for git content (`@/git/`).
pub const DIR_GIT: &str = "git";

/// Directory name for branch listing (`@/git/branches/`).
pub const DIR_BRANCHES: &str = "branches";

/// Directory name for tag listing (`@/git/tags/`).
pub const DIR_TAGS: &str = "tags";

/// Directory name for file history versions (`file.rs@/history/`).
pub const DIR_HISTORY: &str = "history";

/// Directory name for diff variants (`file.rs@/diff/`).
pub const DIR_DIFF: &str = "diff";

/// Virtual file for per-file git blame.
pub const FILE_BLAME: &str = "BLAME.md";

/// Virtual file for per-file git log.
pub const FILE_LOG: &str = "LOG.md";

/// Virtual file for per-file git contributors.
pub const FILE_CONTRIBUTORS: &str = "CONTRIBUTORS.md";

/// Virtual file for per-file git notes.
pub const FILE_NOTES: &str = "NOTES.md";

/// Virtual file for repository-wide git status.
pub const FILE_STATUS: &str = "STATUS.md";

/// Virtual file for HEAD working-directory diff.
pub const FILE_HEAD_DIFF: &str = "HEAD.diff";

/// Create a [`HandleBuilder`](nyne::templates::HandleBuilder) with git name
/// globals pre-registered for template rendering.
pub fn handle_builder() -> HandleBuilder {
    let mut b = HandleBuilder::new();
    let engine = b.engine_mut();
    engine.add_global("DIR_GIT", DIR_GIT);
    engine.add_global("DIR_BRANCHES", DIR_BRANCHES);
    engine.add_global("DIR_TAGS", DIR_TAGS);
    engine.add_global("DIR_HISTORY", DIR_HISTORY);
    engine.add_global("DIR_DIFF", DIR_DIFF);
    engine.add_global("FILE_BLAME", FILE_BLAME);
    engine.add_global("FILE_LOG", FILE_LOG);
    engine.add_global("FILE_CONTRIBUTORS", FILE_CONTRIBUTORS);
    engine.add_global("FILE_NOTES", FILE_NOTES);
    engine.add_global("FILE_STATUS", FILE_STATUS);
    engine.add_global("FILE_HEAD_DIFF", FILE_HEAD_DIFF);
    b
}
