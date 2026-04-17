//! TODO/FIXME comment aggregation configuration.
//!
//! Deserialized from `[plugin.todo]` in the nyne config file. Controls
//! which tags are scanned (e.g. `TODO`, `FIXME`, `HACK`, `XXX`), the
//! maximum number of entries surfaced, and file extensions to exclude.
//! Tags are ordered by priority — the first tag in the list appears
//! first in the overview.

pub mod vfs;
use nyne::plugin::PluginConfig;
use serde::{Deserialize, Serialize};

/// TODO/FIXME comment aggregation configuration.
///
/// Controls which comment tags are scanned across the workspace and in what
/// priority order they appear. The `tags` list is the single source of truth
/// for scanning, grouping, directory layout, and template rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Whether the todo provider is enabled.
    pub enabled: bool,

    /// Tags to scan for, ordered by priority (first = highest).
    /// Case-insensitive matching; canonical case used for display/grouping.
    /// This single list is the SSOT for: scanning, priority, directory names,
    /// .md file generation, and template rendering.
    ///
    /// Order matters: earlier entries are treated as higher severity.
    pub tags: Vec<String>,

    /// Configurable VFS directory and file names.
    pub vfs: vfs::Vfs,
}

/// Default implementation for `Config`.
impl Default for Config {
    /// Returns the default value.
    fn default() -> Self {
        Self {
            enabled: true,
            tags: ["FIXME", "SAFETY", "HACK", "XXX", "TODO"]
                .into_iter()
                .map(Into::into)
                .collect(),
            vfs: vfs::Vfs::default(),
        }
    }
}

impl PluginConfig for Config {}

#[cfg(test)]
mod tests;
