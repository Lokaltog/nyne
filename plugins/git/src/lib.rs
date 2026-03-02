//! Git repository integration for nyne.
//!
//! Provides [`GitRepo`] for git operations and `GitPlugin`
//! which inserts the repo into the activation context's `TypeMap`.

mod commit;
mod companion;
pub mod names;
mod plugin;
pub mod provider;
pub mod repo;
mod status;

pub use commit::{CommitInfo, commit_info, diff_opts};
pub use repo::GitRepo;
pub use status::RepoStatus;
