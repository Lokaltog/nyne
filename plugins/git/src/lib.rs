//! Git repository integration for nyne.
//!
//! Provides [`GitRepo`] for git operations and `GitPlugin`
//! which inserts the repo into the activation context's `TypeMap`.

/// Git-backed project cloning for overlay lowerdirs.
mod clone;
/// Commit metadata types.
mod commit;
/// Git-aware companion provider.
mod companion;
/// Route and template name constants.
pub mod names;
/// Plugin registration entry point.
mod plugin;
/// Git VFS provider implementation.
pub mod provider;
/// Git repository wrapper.
pub mod repo;
/// Working tree status types.
mod status;

pub use commit::CommitInfo;
pub use provider::history::HistoryQueries;
pub use provider::repo::FileViewCtx;
pub use provider::views::{BLAME_TEMPLATE, HISTORY_LIMIT, LOG_TEMPLATE, history_filename, hunk_overlaps_range};
pub use provider::{CommitMtime, CommitMtimeExt};
pub use repo::GitRepo;
pub use status::RepoStatus;
