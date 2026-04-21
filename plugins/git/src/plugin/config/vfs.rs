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

nyne::vfs_struct! {
    /// Configurable directory names for the git plugin.
    pub struct VfsDirs {
        /// Top-level git content directory.
        git = "git",
        /// Branch listing subdirectory.
        branches = "branches",
        /// Tag listing subdirectory.
        tags = "tags",
        /// File history versions subdirectory.
        history = "history",
        /// Diff variants subdirectory.
        diff = "diff",
    }
}

nyne::vfs_struct! {
    /// Configurable file names for the git plugin.
    pub struct VfsFiles {
        /// Per-file git blame.
        blame = "BLAME.md",
        /// Per-file git log.
        log = "LOG.md",
        /// Per-file git contributors.
        contributors = "CONTRIBUTORS.md",
        /// Per-file git notes.
        notes = "NOTES.md",
        /// Repository-wide git status.
        status = "STATUS.md",
        /// HEAD working-directory diff.
        head_diff = "HEAD.diff",
    }
}
