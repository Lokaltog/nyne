//! TODO/FIXME comment aggregation configuration.

use serde::{Deserialize, Serialize};

use super::default_true;

/// TODO/FIXME comment aggregation configuration.
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

impl Default for TodoConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            tags: default_todo_tags(),
        }
    }
}

fn default_todo_tags() -> Vec<String> {
    ["FIXME", "SAFETY", "HACK", "XXX", "TODO"]
        .into_iter()
        .map(String::from)
        .collect()
}
