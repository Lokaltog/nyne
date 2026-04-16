pub mod vfs;

use nyne::config::PluginConfig;
use serde::{Deserialize, Serialize};

/// Top-level configuration for the git plugin.
///
/// ```toml
/// [plugin.git.limits]
/// history = 50
/// log = 200
/// notes = 50
/// contributors = 500
/// recent_commits = 10
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
    /// Caps on the number of entries rendered into git virtual files.
    pub limits: Limits,

    /// Configurable VFS directory and file names.
    pub vfs: vfs::Vfs,
}

/// Caps on the number of entries rendered into git virtual files.
///
/// All limits are per-file (except `recent_commits`, which applies to
/// the repository-wide `STATUS.md`). Set high enough that content
/// remains useful but bounded to keep rendering fast.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    /// Maximum commits rendered into `file.rs@/history/` and
    /// `file.rs@/git/history/`.
    pub history: usize,

    /// Maximum commits rendered into `file.rs@/git/LOG.md`.
    pub log: usize,

    /// Maximum commits scanned for notes rendered into
    /// `file.rs@/git/NOTES.md`.
    pub notes: usize,

    /// Maximum commits examined when computing contributors for a file.
    pub contributors: usize,

    /// Number of recent commits shown in `@/git/STATUS.md`.
    pub recent_commits: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { history: 50, log: 200, notes: 50, contributors: 500, recent_commits: 10 }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self { limits: Limits::default(), vfs: vfs::Vfs::default() }
    }
}

impl PluginConfig for Config {}

#[cfg(test)]
mod tests;
