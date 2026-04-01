pub mod vfs;

use nyne::config::PluginConfig;
use serde::{Deserialize, Serialize};

/// Top-level configuration for the git plugin.
///
/// ```toml
/// [plugin.git]
/// history_limit = 50
///
/// [plugin.git.vfs.dir]
/// git = "git"
/// branches = "branches"
///
/// [plugin.git.vfs.file]
/// blame = "BLAME.md"
/// log = "LOG.md"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Maximum number of history entries shown per file.
    pub history_limit: usize,

    /// Configurable VFS directory and file names.
    pub vfs: vfs::Vfs,
}

const fn default_history_limit() -> usize { 50 }

impl Default for Config {
    fn default() -> Self {
        Self {
            history_limit: default_history_limit(),
            vfs: vfs::Vfs::default(),
        }
    }
}

impl PluginConfig for Config {}

#[cfg(test)]
mod tests;
