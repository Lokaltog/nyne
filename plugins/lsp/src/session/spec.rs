//! LSP server definition -- runtime representation of a config-driven server entry.
//!
//! [`ServerDef`] is built from resolved [`ServerEntry`] config during
//! [`Registry`](super::Registry) construction. It holds the command,
//! arguments, and root markers needed to spawn and gate a language server.
//! This is the boundary between config types (serde) and runtime types
//! (spawn logic).

use std::path::Path;
use std::time::Duration;

use crate::plugin::config::{ServerEntry, default_server_index_debounce};

/// Runtime definition of an LSP server.
///
/// Built from resolved [`ServerEntry`] config during registry construction.
#[derive(Clone)]
pub struct ServerDef {
    /// Unique server name (e.g., "rust-analyzer").
    name: String,
    /// Command to spawn. Defaults to name.
    command: String,
    /// Arguments passed to the server process.
    args: Vec<String>,
    /// Project marker files — server applicable if any exist in root.
    /// Empty = always applicable.
    root_markers: Vec<String>,
    /// Quiescence window applied by the indexing-progress gate before
    /// transitioning to `Ready`. Resolved from config (or the default)
    /// at registry-construction time so spawn paths don't re-resolve.
    index_debounce: Duration,
}

/// Construction, accessors, and project-root applicability checks.
impl ServerDef {
    /// Build from a server name and its config entry.
    pub(crate) fn from_entry(name: &str, entry: &ServerEntry) -> Self {
        Self {
            command: entry.command.clone().unwrap_or_else(|| name.to_owned()),
            name: name.to_owned(),
            args: entry.args.clone().unwrap_or_default(),
            root_markers: entry.root_markers.clone().unwrap_or_default(),
            index_debounce: entry.index_debounce.unwrap_or_else(default_server_index_debounce),
        }
    }

    /// Server identifier name.
    pub(crate) fn name(&self) -> &str { &self.name }

    /// Executable command to spawn this server.
    pub(crate) fn command_str(&self) -> &str { &self.command }

    /// Command-line arguments passed to the server on spawn.
    pub(crate) fn args_slice(&self) -> &[String] { &self.args }

    /// Quiescence window applied by the indexing-progress gate before
    /// transitioning to `Ready` for clients spawned from this def.
    pub(crate) const fn index_debounce(&self) -> Duration { self.index_debounce }

    /// Check whether this server is applicable for the given project root.
    pub(crate) fn is_applicable(&self, root: &Path) -> bool {
        if self.root_markers.is_empty() {
            return true;
        }
        let result = self.root_markers.iter().any(|f| root.join(f).exists());
        if !result {
            tracing::debug!(
                target: "nyne::lsp",
                server = %self.name,
                root = %root.display(),
                "no root markers found — server not applicable for this project",
            );
        }
        result
    }
}
