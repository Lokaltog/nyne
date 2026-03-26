use std::path::Path;

use crate::config::ServerEntry;

/// Runtime definition of an LSP server.
///
/// Built from resolved [`ServerEntry`] config during registry construction.
#[derive(Clone)]
pub struct LspServerDef {
    /// Unique server name (e.g., "rust-analyzer").
    name: String,
    /// Command to spawn. Defaults to name.
    command: String,
    /// Arguments passed to the server process.
    args: Vec<String>,
    /// Project marker files — server applicable if any exist in root.
    /// Empty = always applicable.
    root_markers: Vec<String>,
}

impl LspServerDef {
    /// Build from a resolved server entry (all required fields must be present).
    pub(crate) fn from_entry(entry: &ServerEntry) -> Self {
        Self {
            command: entry.command.clone().unwrap_or_else(|| entry.name.clone()),
            name: entry.name.clone(),
            args: entry.args.clone().unwrap_or_default(),
            root_markers: entry.root_markers.clone().unwrap_or_default(),
        }
    }

    /// Server identifier name.
    pub(crate) fn name(&self) -> &str { &self.name }

    /// Executable command to spawn this server.
    pub(crate) fn command_str(&self) -> &str { &self.command }

    /// Command-line arguments passed to the server on spawn.
    pub(crate) fn args_slice(&self) -> &[String] { &self.args }

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
