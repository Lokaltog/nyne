use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use strum::Display;

/// Controls virtual filesystem visibility for a specific process (and its descendants).
///
/// Set via `nyne attach --visibility` or dynamically via
/// [`ControlRequest::SetVisibility`](crate::sandbox::control::ControlRequest::SetVisibility).
///
/// Resolution order per FUSE request:
/// 1. Direct PID lookup in the [`VisibilityMap`](super::VisibilityMap).
/// 2. Name-based rule (process comm matched against configured names).
/// 3. Falls back to [`Default`](ProcessVisibility::Default).
#[derive(Debug, Display, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum ProcessVisibility {
    /// Force all directly-resolvable nodes into readdir listings.
    ///
    /// Nodes with [`Visibility::Hidden`](crate::node::Visibility::Hidden)
    /// (e.g., companion `@/` directories) become visible. Lookup-only
    /// dynamic nodes (e.g., `lines:M-N`) remain excluded.
    All,

    /// Normal nyne behavior — hidden nodes accessible by name lookup only.
    Default,

    /// Full passthrough — process sees only the real filesystem.
    ///
    /// The process never sees virtual nodes, forced replacements, or
    /// companion namespaces. This is the same behavior that
    /// `passthrough_processes` applies to git and LSP servers.
    None,
}
