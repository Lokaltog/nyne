//! Configuration for the code analysis engine.

use std::collections::HashSet;

use nyne::default_true;
use serde::{Deserialize, Serialize};

/// Configuration for the code analysis engine.
///
/// ```toml
/// [plugin.analysis]
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
    /// - `None` (absent from config) -> all rules except default-disabled noisy rules.
    /// - `Some([])` (explicit empty) -> all registered rules, no exclusions.
    /// - `Some(set)` -> only rules whose `id()` matches an entry.
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

impl AnalysisConfig {
    /// Deserialize from the plugin config section, falling back to defaults.
    pub fn from_plugin_config(section: Option<&serde_json::Value>) -> Self {
        let Some(value) = section else {
            return Self::default();
        };
        serde_json::from_value(value.clone()).unwrap_or_default()
    }
}
