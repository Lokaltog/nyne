//! Source plugin configuration types and deserialization.
//!
//! All config lives under `[plugin.source]` in `config.toml`. The top-level
//! struct is [`SourceConfig`].
//! Serde's `deny_unknown_fields` is applied on every struct to catch typos early.

use nyne::config::deserialize_plugin_config;
use serde::{Deserialize, Serialize};

/// Top-level configuration for the source plugin.
///
/// Deserialized from the `[plugin.source]` section of `config.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceConfig {}

/// Deserialization and config loading methods.
impl SourceConfig {
    /// Deserialize from the plugin config section, falling back to defaults.
    pub fn from_plugin_config(section: Option<&serde_json::Value>) -> Self {
        let Some(value) = section else {
            return Self::default();
        };
        deserialize_plugin_config(value)
    }
}

/// Unit tests for coding plugin configuration.
#[cfg(test)]
mod tests;
