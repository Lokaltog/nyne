//! Git repository integration for nyne.
//!
//! Provides [`GitRepo`] for git operations and `GitPlugin`
//! which inserts the repo into the activation context's `TypeMap`.

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

pub use commit::{CommitInfo, commit_info, diff_opts};
pub use repo::GitRepo;
pub use status::RepoStatus;
