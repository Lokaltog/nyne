//! LSP configuration types.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::default_true;

/// Top-level LSP configuration.
///
/// Built-in server defaults come from `register_lsp!` declarations.
/// This config provides overrides and additions only — users never
/// need to re-declare built-in knowledge.
///
/// Always present in `CodingConfig` (via `#[serde(default)]`).
/// LSP is enabled by default; set `enabled = false` to disable.
///
/// Deserialized from the `[lsp]` table (top-level, not under `[plugin.coding]`)
/// so that LSP settings live alongside language server tooling rather than
/// being buried inside plugin-specific config.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LspConfig {
    /// Whether LSP integration is enabled at all.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Cache TTL for LSP query results.
    #[serde(default = "default_lsp_cache_ttl")]
    #[serde(with = "humantime_serde")]
    pub cache_ttl: Duration,

    /// Timeout for waiting on LSP diagnostics after a write.
    #[serde(default = "default_diagnostics_timeout")]
    #[serde(with = "humantime_serde")]
    pub diagnostics_timeout: Duration,

    /// Timeout for individual LSP request-response cycles.
    ///
    /// Guards against deadlocks where an LSP server stops responding
    /// (e.g., blocked on unhandled protocol messages). Applies to all
    /// `send_request` calls including the initialize handshake.
    #[serde(default = "default_response_timeout")]
    #[serde(with = "humantime_serde")]
    pub response_timeout: Duration,

    /// Override built-in server configurations by name.
    /// Keys must match a server name from `register_lsp!` declarations.
    #[serde(default)]
    pub servers: HashMap<String, LspServerOverride>,

    /// Additional custom LSP servers not covered by built-in declarations.
    #[serde(default)]
    pub custom: Vec<CustomLspServer>,

    /// Maximum number of results returned by workspace symbol search.
    #[serde(default = "default_workspace_symbol_limit")]
    pub workspace_symbol_limit: usize,
}

/// Default implementation for `LspConfig`.
impl Default for LspConfig {
    /// Returns the default value.
    fn default() -> Self {
        Self {
            enabled: default_true(),
            cache_ttl: default_lsp_cache_ttl(),
            diagnostics_timeout: default_diagnostics_timeout(),
            response_timeout: default_response_timeout(),
            servers: HashMap::new(),
            custom: Vec::new(),
            workspace_symbol_limit: default_workspace_symbol_limit(),
        }
    }
}

/// Override properties of a built-in LSP server.
///
/// Only specified fields are overridden; omitted fields keep their defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LspServerOverride {
    /// Set to `false` to disable this server entirely.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Override the server command.
    pub command: Option<String>,
    /// Override the server arguments.
    pub args: Option<Vec<String>>,
}

/// A custom LSP server defined entirely in config.
///
/// Unlike built-in servers, custom servers have no detection function —
/// they're always considered applicable for their declared extensions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomLspServer {
    /// Unique name for this server.
    pub name: String,
    /// Command to spawn.
    pub command: String,
    /// Arguments passed to the server process.
    #[serde(default)]
    pub args: Vec<String>,
    /// File extensions this server handles.
    pub extensions: Vec<String>,
}

/// Default cache TTL for LSP query results (5 minutes).
const fn default_lsp_cache_ttl() -> Duration {
    Duration::from_secs(300) // 5 minutes
}

/// Default timeout for waiting on LSP diagnostics after a write (2 seconds).
const fn default_diagnostics_timeout() -> Duration { Duration::from_secs(2) }

/// Default timeout for individual LSP request-response cycles (10 seconds).
const fn default_response_timeout() -> Duration { Duration::from_secs(10) }

/// Default maximum number of results for workspace symbol search.
const fn default_workspace_symbol_limit() -> usize { 20 }
