//! Git repository integration for nyne.
//!
//! Provides [`Repo`] for git operations and `GitPlugin`
//! which inserts the repo into the activation context's `AnyMap`.

/// Git-backed project cloning for overlay lowerdirs.
mod clone;
/// Commit metadata types.
mod commit;
/// Extension trait for accessing git services from `ActivationContext`.
pub(crate) mod context;
/// Git history queries (blame, file history, contributors, notes).
mod history;
/// Plugin registration entry point.
mod plugin;
/// Git VFS provider implementation.
pub(crate) mod provider;
/// Git repository wrapper.
pub(crate) mod repo;
/// Working tree status types.
mod status;

pub use commit::CommitInfo;
pub use context::GitContextExt;
pub use provider::GitProvider;
pub use repo::Repo;
pub use status::RepoStatus;
