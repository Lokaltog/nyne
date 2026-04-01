//! Extension trait for accessing git plugin services from [`ActivationContext`].

use std::sync::Arc;

use nyne::ExtensionCounts;

use crate::provider::GitState;
use crate::repo::Repo;

nyne::activation_context_ext! {
    /// Typed accessors for git plugin services in [`ActivationContext`].
    ///
    /// All methods return `Option` because git services are only available
    /// when a git repository is detected during activation.
    pub trait GitContextExt {
        /// The git repository handle.
        git_repo -> Arc<Repo>,
        /// Shared git state (branches, HEAD, etc.).
        git_state -> Arc<GitState>,
        /// VFS extension counts (companion namespace entry counts per directory).
        extension_counts -> ExtensionCounts,
    }
}
