/// Extension trait for accessing visibility services from `ActivationContext`.
pub(crate) mod context;
/// Visibility middleware — sets visibility state and post-filters virtual nodes.
pub(crate) mod provider;

/// Per-process visibility levels (All / Default / None).
pub(crate) mod process_visibility;

/// Per-process visibility resolution map (PID overrides, name rules, cgroups).
pub(crate) mod visibility_map;

/// Additional passthrough process names contributed by other plugins.
mod passthrough;
pub use context::VisibilityContextExt;
pub use passthrough::PassthroughProcesses;
pub use provider::{Visibility, VisibilityRequest};

/// SetVisibility control command handler.
pub(crate) mod control;

mod plugin;
