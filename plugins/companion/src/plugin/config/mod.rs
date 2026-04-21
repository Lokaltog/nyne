//! Configuration for `[plugin.companion]`.

use serde::{Deserialize, Serialize};

/// Companion plugin configuration.
///
/// ```toml
/// [plugin.companion]
/// suffix = "@"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CompanionConfig {
    /// Suffix appended to filenames to form companion directories.
    ///
    /// Defaults to `"@"` — e.g., `foo.rs` → `foo.rs@/`.
    pub suffix: String,
}

fn default_suffix() -> String { "@".into() }

impl Default for CompanionConfig {
    fn default() -> Self {
        Self {
            suffix: default_suffix(),
        }
    }
}
