//! Cross-plugin communication type for passthrough process names.
//!
//! Plugins (e.g., LSP) insert [`PassthroughProcesses`] into the
//! [`ActivationContext`](nyne::dispatch::activation::ActivationContext)
//! during activation. The visibility plugin reads it when building the
//! [`VisibilityMap`](crate::visibility_map::VisibilityMap).

/// Additional passthrough process names contributed by plugins at activation time.
///
/// Inserted into the `AnyMap` by plugins (e.g., the LSP plugin adds LSP
/// server commands). The visibility plugin merges these with its own
/// `passthrough_processes` config when building the visibility map.
#[derive(Debug, Clone, Default)]
pub struct PassthroughProcesses(Vec<String>);

impl PassthroughProcesses {
    /// Wrap a list of process names that should receive raw filesystem passthrough.
    pub const fn new(names: Vec<String>) -> Self { Self(names) }

    /// Borrow the process name list.
    pub fn as_slice(&self) -> &[String] { &self.0 }
}
