use nyne::templates::TemplateGlobals;
use serde::{Deserialize, Serialize};

/// VFS path configuration for the analysis plugin.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Vfs {
    /// File names in the analysis VFS tree.
    pub file: VfsFiles,
}

impl TemplateGlobals for Vfs {}

nyne::vfs_struct! {
    /// Configurable file names for the analysis plugin.
    pub struct VfsFiles {
        /// Per-symbol analysis hints file name.
        analysis = "ANALYSIS.md",
    }
}
