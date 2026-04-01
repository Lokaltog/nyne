//! LSP configuration types.

pub mod vfs;

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use nyne::config::PluginConfig;
use serde::{Deserialize, Serialize};

/// Maps file extensions to LSP language identifiers.
///
/// A plain string applies the same ID to all extensions. A table provides
/// per-extension mapping for languages like TypeScript where `.ts` and
/// `.tsx` need different identifiers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LanguageIdMapping {
    /// Single language ID for all extensions (e.g., `"rust"`).
    Uniform(String),
    /// Per-extension mapping (e.g., `{ ts = "typescript", tsx = "typescriptreact" }`).
    PerExtension(HashMap<String, String>),
}

impl LanguageIdMapping {
    /// Resolve the language ID for a specific file extension.
    ///
    /// Returns `None` if the mapping is per-extension and has no entry for the given extension.
    pub fn resolve(&self, ext: &str) -> Option<&str> {
        match self {
            Self::Uniform(id) => Some(id.as_str()),
            Self::PerExtension(map) => map.get(ext).map(String::as_str),
        }
    }
}

/// LSP server entry in config.
///
/// When overriding a built-in server, only `name` is required — omitted
/// fields inherit from the default via config merge. When defining a new
/// server, `extensions` and `language_ids` are required.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerEntry {
    /// Executable to spawn. Defaults to the map key (server name) if omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    /// Command-line arguments passed to the server process.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,

    /// File extensions this server handles.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,

    /// Extension → LSP language identifier mapping.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language_ids: Option<LanguageIdMapping>,

    /// Project marker files — server is applicable if any exist in the
    /// project root. Empty or omitted means always applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_markers: Option<Vec<String>>,

    /// Whether this server is enabled. Set to `false` to disable a
    /// built-in server.
    pub enabled: bool,
}

impl Default for ServerEntry {
    fn default() -> Self {
        Self {
            command: None,
            args: None,
            extensions: None,
            language_ids: None,
            root_markers: None,
            enabled: true,
        }
    }
}

/// Top-level LSP configuration.
///
/// Built-in server defaults come from [`default_servers`]. User and
/// project config layers override via the merge chain.
///
/// Always present in `SourceConfig` (via `#[serde(default)]`).
/// LSP is enabled by default; set `enabled = false` to disable.
///
/// Deserialized from the `[lsp]` table (top-level, not under `[plugin.source]`)
/// so that LSP settings live alongside language server tooling rather than
/// being buried inside plugin-specific config.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Whether LSP integration is enabled at all.
    pub(crate) enabled: bool,

    /// Cache TTL for LSP query results.
    #[serde(with = "humantime_serde")]
    pub(crate) cache_ttl: Duration,

    /// Timeout for waiting on LSP diagnostics after a write.
    #[serde(with = "humantime_serde")]
    pub(crate) diagnostics_timeout: Duration,

    /// Timeout for individual LSP request-response cycles.
    ///
    /// Guards against deadlocks where an LSP server stops responding
    /// (e.g., blocked on unhandled protocol messages). Applies to all
    /// `send_request` calls including the initialize handshake.
    #[serde(with = "humantime_serde")]
    pub(crate) response_timeout: Duration,

    /// LSP server definitions, keyed by server name.
    ///
    /// Built-in defaults come from [`default_servers`]. User and project
    /// config layers merge by key — later layers override per-field.
    pub(crate) servers: BTreeMap<String, ServerEntry>,

    /// Maximum number of results returned by workspace symbol search.
    pub(crate) workspace_symbol_limit: usize,

    /// Configurable VFS directory and file names.
    pub vfs: vfs::Vfs,
}
impl PluginConfig for Config {}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            cache_ttl: default_lsp_cache_ttl(),
            diagnostics_timeout: default_diagnostics_timeout(),
            response_timeout: default_response_timeout(),
            servers: default_servers(),
            workspace_symbol_limit: default_workspace_symbol_limit(),
            vfs: vfs::Vfs::default(),
        }
    }
}

/// Default cache TTL for LSP query results (5 minutes).
const fn default_lsp_cache_ttl() -> Duration {
    Duration::from_mins(5) // 5 minutes
}

/// Default timeout for waiting on LSP diagnostics after a write (2 seconds).
const fn default_diagnostics_timeout() -> Duration { Duration::from_secs(2) }

/// Default timeout for individual LSP request-response cycles (10 seconds).
const fn default_response_timeout() -> Duration { Duration::from_secs(10) }

/// Default maximum number of results for workspace symbol search.
const fn default_workspace_symbol_limit() -> usize { 20 }
/// Concise constructor for a `(name, ServerEntry)` pair.
///
/// ```ignore
/// server!("rust-analyzer",
///     extensions: ["rs"],
///     language_ids: "rust",
///     root_markers: ["Cargo.toml"],
/// )
/// ```
macro_rules! server {
    // Internal rule: accumulate keyword args into field assignments.
    (@fields $entry:ident,) => {};

    (@fields $entry:ident, command: $cmd:expr, $($rest:tt)*) => {
        $entry.command = Some($cmd.into());
        server!(@fields $entry, $($rest)*);
    };
    (@fields $entry:ident, args: [$($arg:expr),* $(,)?], $($rest:tt)*) => {
        $entry.args = Some(vec![$($arg.into()),*]);
        server!(@fields $entry, $($rest)*);
    };
    (@fields $entry:ident, extensions: [$($ext:expr),* $(,)?], $($rest:tt)*) => {
        $entry.extensions = Some(vec![$($ext.into()),*]);
        server!(@fields $entry, $($rest)*);
    };
    (@fields $entry:ident, language_ids: {$($ext:literal => $id:literal),* $(,)?}, $($rest:tt)*) => {
        $entry.language_ids = Some(LanguageIdMapping::PerExtension(HashMap::from([
            $(($ext.into(), $id.into())),*
        ])));
        server!(@fields $entry, $($rest)*);
    };
    (@fields $entry:ident, language_ids: $lang:literal, $($rest:tt)*) => {
        $entry.language_ids = Some(LanguageIdMapping::Uniform($lang.into()));
        server!(@fields $entry, $($rest)*);
    };
    (@fields $entry:ident, root_markers: [$($marker:expr),* $(,)?], $($rest:tt)*) => {
        $entry.root_markers = Some(vec![$($marker.into()),*]);
        server!(@fields $entry, $($rest)*);
    };

    // Entry point: name is required positional, rest are keyword args.
    ($name:expr, $($fields:tt)*) => {{
        #[allow(unused_mut)]
        let mut entry = ServerEntry::default();
        server!(@fields entry, $($fields)*);
        (String::from($name), entry)
    }};
}
/// Built-in LSP server definitions.
///
/// These are the lowest-priority defaults, overridable by user and project
/// config via the merge chain.
fn default_servers() -> BTreeMap<String, ServerEntry> {
    BTreeMap::from([
        server!("rust-analyzer",
            extensions: ["rs"],
            language_ids: "rust",
            root_markers: ["Cargo.toml"],
        ),
        server!("tsgo",
            args: ["--lsp", "--stdio"],
            extensions: ["ts", "tsx"],
            language_ids: {"ts" => "typescript", "tsx" => "typescriptreact"},
            root_markers: ["package.json"],
        ),
        server!("typescript-language-server",
            args: ["--stdio"],
            extensions: ["ts", "tsx"],
            language_ids: {"ts" => "typescript", "tsx" => "typescriptreact"},
            root_markers: ["package.json"],
        ),
        server!("basedpyright",
            command: "basedpyright-langserver",
            args: ["--stdio"],
            extensions: ["py"],
            language_ids: "python",
            root_markers: ["pyproject.toml"],
        ),
    ])
}

#[cfg(test)]
mod tests;
