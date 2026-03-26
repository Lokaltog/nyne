//! TODO/FIXME comment aggregation configuration.
//!
//! Deserialized from `[plugin.todo]` in the nyne config file. Controls
//! which tags are scanned (e.g. `TODO`, `FIXME`, `HACK`, `XXX`), the
//! maximum number of entries surfaced, and file extensions to exclude.
//! Tags are ordered by priority — the first tag in the list appears
//! first in the overview.

use nyne::config::deserialize_plugin_config;
use nyne::default_true;
use serde::{Deserialize, Serialize};

/// TODO/FIXME comment aggregation configuration.
///
/// Controls which comment tags are scanned across the workspace and in what
/// priority order they appear. The `tags` list is the single source of truth
/// for scanning, grouping, directory layout, and template rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Whether the todo provider is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Tags to scan for, ordered by priority (first = highest).
    /// Case-insensitive matching; canonical case used for display/grouping.
    /// This single list is the SSOT for: scanning, priority, directory names,
    /// .md file generation, and template rendering.
    #[serde(default = "default_todo_tags")]
    pub tags: Vec<String>,
}

/// Default implementation for `Config`.
impl Default for Config {
    /// Returns the default value.
    fn default() -> Self {
        Self {
            enabled: true,
            tags: default_todo_tags(),
        }
    }
}

/// Deserialization methods for [`Config`].
impl Config {
    /// Deserialize from the plugin config section, falling back to defaults.
    pub fn from_plugin_config(section: Option<&serde_json::Value>) -> Self {
        let Some(value) = section else {
            return Self::default();
        };
        deserialize_plugin_config(value)
    }
}

/// Default set of TODO tags ordered by priority.
///
/// Order matters: earlier entries are treated as higher severity. `FIXME`
/// comes first because it implies something is broken, while `TODO` is
/// lowest because it is purely aspirational.
fn default_todo_tags() -> Vec<String> {
    ["FIXME", "SAFETY", "HACK", "XXX", "TODO"]
        .into_iter()
        .map(Into::into)
        .collect()
}
