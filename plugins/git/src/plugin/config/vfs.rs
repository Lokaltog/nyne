use nyne::templates::TemplateGlobals;
use serde::{Deserialize, Serialize};

/// VFS path configuration for the git plugin.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Vfs {
    /// Directory names in the git VFS tree.
    pub dir: VfsDirs,

    /// File names in the git VFS tree.
    pub file: VfsFiles,
}

impl TemplateGlobals for Vfs {}

/// Configurable directory names for the git plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VfsDirs {
    /// Top-level git content directory.
    pub git: String,

    /// Branch listing subdirectory.
    pub branches: String,

    /// Tag listing subdirectory.
    pub tags: String,

    /// File history versions subdirectory.
    pub history: String,

    /// Diff variants subdirectory.
    pub diff: String,
}

impl Default for VfsDirs {
    fn default() -> Self {
        Self {
            git: "git".into(),
            branches: "branches".into(),
            tags: "tags".into(),
            history: "history".into(),
            diff: "diff".into(),
        }
    }
}

/// Configurable file names for the git plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VfsFiles {
    /// Per-file git blame.
    pub blame: String,

    /// Per-file git log.
    pub log: String,

    /// Per-file git contributors.
    pub contributors: String,

    /// Per-file git notes.
    pub notes: String,

    /// Repository-wide git status.
    pub status: String,

    /// HEAD working-directory diff.
    pub head_diff: String,
}

impl Default for VfsFiles {
    fn default() -> Self {
        Self {
            blame: "BLAME.md".into(),
            log: "LOG.md".into(),
            contributors: "CONTRIBUTORS.md".into(),
            notes: "NOTES.md".into(),
            status: "STATUS.md".into(),
            head_diff: "HEAD.diff".into(),
        }
    }
}
