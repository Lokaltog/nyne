//! Configuration for the code analysis engine.

pub mod vfs;
use std::collections::HashSet;

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
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Global kill switch for all code analysis. Default: `true`.
    pub enabled: bool,

    /// Which analysis rules to activate.
    ///
    /// - `None` (absent from config) -> all rules except default-disabled noisy rules.
    /// - `Some([])` (explicit empty) -> all registered rules, no exclusions.
    /// - `Some(set)` -> only rules whose `id()` matches an entry.
    ///   Unknown names produce a warning at startup.
    pub rules: Option<HashSet<String>>,

    /// Configurable VFS file names.
    pub vfs: vfs::Vfs,
}

/// Default implementation for `Config`.
impl Default for Config {
    /// Returns the default value.
    fn default() -> Self {
        Self {
            enabled: true,
            rules: None,
            vfs: vfs::Vfs::default(),
        }
    }
}

#[cfg(test)]
mod tests;
