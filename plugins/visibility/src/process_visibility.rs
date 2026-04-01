//! Per-process visibility levels controlling VFS content filtering.
//!
//! Each process attached to a nyne sandbox can have its own visibility level,
//! determining whether it sees virtual nodes, hidden companion directories,
//! or only the raw real filesystem. This allows tools like git and LSP servers
//! to operate on the real files while interactive shells see the full VFS.

use serde::{Deserialize, Serialize};
use strum::Display;

/// Controls virtual filesystem visibility for a specific process (and its descendants).
///
/// Set dynamically via the `SetVisibility` control command or via
/// `passthrough_processes` in `[plugin.visibility]` config.
///
/// Resolution order per FUSE request:
/// 1. Direct PID lookup in the [`VisibilityMap`](crate::visibility_map::VisibilityMap).
/// 2. Name-based rule (process comm matched against configured names).
/// 3. Falls back to [`Default`](ProcessVisibility::Default).
#[derive(Debug, Display, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum ProcessVisibility {
    /// Force all directly-resolvable nodes into readdir listings.
    ///
    /// Companion `@/` directories become visible in listings. Lookup-only
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
