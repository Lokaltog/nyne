//! Coding plugin configuration types and deserialization.
//!
//! All config lives under `[plugin.coding]` in `config.toml`. The top-level
//! struct is [`CodingConfig`], which nests sub-configs for LSP and analysis.
//! Serde's `deny_unknown_fields` is applied on every struct to catch typos early.

/// LSP configuration types.
pub mod lsp;

use std::collections::HashSet;

use nyne::default_true;
use serde::{Deserialize, Serialize};

use self::lsp::LspConfig;

/// Top-level configuration for the coding plugin.
///
/// Deserialized from the `[plugin.coding]` section of `config.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodingConfig {
    /// LSP configuration.
    #[serde(default)]
    pub lsp: LspConfig,

    /// Code analysis configuration.
    #[serde(default)]
    pub analysis: AnalysisConfig,
}

/// Configuration for the code analysis engine.
///
/// ```toml
/// [plugin.coding.analysis]
/// enabled = true
/// # Absent or omitted: all rules except noisy defaults (magic-string, magic-number).
/// # Explicit empty: all rules, no exclusions.
/// # rules = []
/// # Specific set: only these rules.
/// # rules = ["deep-nesting", "empty-catch", "unwrap-chain"]
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisConfig {
    /// Global kill switch for all code analysis. Default: `true`.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Which analysis rules to activate.
    ///
    /// - `None` (absent from config) → all rules except default-disabled noisy rules.
    /// - `Some([])` (explicit empty) → all registered rules, no exclusions.
    /// - `Some(set)` → only rules whose `id()` matches an entry.
    ///   Unknown names produce a warning at startup.
    pub rules: Option<HashSet<String>>,
}

/// Default implementation for `AnalysisConfig`.
impl Default for AnalysisConfig {
    /// Returns the default value.
    fn default() -> Self {
        Self {
            enabled: true,
            rules: None,
        }
    }
}

/// Deserialization and config loading methods.
impl CodingConfig {
    /// Deserialize from the plugin config section, falling back to defaults.
    pub fn from_plugin_config(section: Option<&serde_json::Value>) -> Self {
        let Some(value) = section else {
            return Self::default();
        };
        serde_json::from_value(value.clone()).unwrap_or_default()
    }
}

/// Unit tests for coding plugin configuration.
#[cfg(test)]
mod tests;
