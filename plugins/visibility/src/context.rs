//! Extension trait for accessing visibility plugin services from [`ActivationContext`].

use std::sync::Arc;

use crate::PassthroughProcesses;
use crate::visibility_map::VisibilityMap;

nyne::activation_context_ext! {
    /// Typed accessors for visibility plugin services in [`ActivationContext`].
    pub trait VisibilityContextExt {
        /// The per-process visibility resolution map.
        visibility_map -> Arc<VisibilityMap>,
        /// Additional passthrough process names (contributed by the LSP plugin).
        passthrough_processes -> PassthroughProcesses,
    }
}
