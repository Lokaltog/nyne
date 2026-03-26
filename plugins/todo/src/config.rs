//! TODO/FIXME comment aggregation configuration.

use nyne::default_true;
use serde::{Deserialize, Serialize};

/// TODO/FIXME comment aggregation configuration.
///
/// Controls which comment tags are scanned across the workspace and in what
/// priority order they appear. The `tags` list is the single source of truth
/// for scanning, grouping, directory layout, and template rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TodoConfig {
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

/// Default implementation for `TodoConfig`.
impl Default for TodoConfig {
    /// Returns the default value.
    fn default() -> Self {
        Self {
            enabled: true,
            tags: default_todo_tags(),
        }
    }
}

impl TodoConfig {
    /// Deserialize from the plugin config section, falling back to defaults.
    pub fn from_plugin_config(section: Option<&serde_json::Value>) -> Self {
        let Some(value) = section else {
            return Self::default();
        };
        serde_json::from_value(value.clone()).unwrap_or_default()
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
