//! Source plugin configuration types and deserialization.
//!
//! All config lives under `[plugin.source]` in `config.toml`. The top-level
//! struct is [`Config`].
//! Serde's `deny_unknown_fields` is applied on every struct to catch typos early.

pub mod vfs;

use nyne::config::PluginConfig;
use serde::{Deserialize, Serialize};

const fn default_max_depth() -> usize { 5 }

/// Top-level configuration for the source plugin.
///
/// ```toml
/// [plugin.source]
/// enabled = true
/// max_depth = 5
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Global kill switch for source decomposition. Default: `true`.
    pub enabled: bool,

    /// Maximum nesting depth for recursive symbol extraction.
    ///
    /// Deeper nesting is flattened into the parent fragment's body.
    /// Default: `5` (handles module > impl > fn > closure patterns).
    /// Increase for deeply nested codebases (Lua callbacks, JS builder patterns).
    pub max_depth: usize,

    /// Configurable VFS directory and file names.
    pub vfs: vfs::Vfs,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            max_depth: default_max_depth(),
            vfs: vfs::Vfs::default(),
        }
    }
}

impl PluginConfig for Config {}

#[cfg(test)]
mod tests;
