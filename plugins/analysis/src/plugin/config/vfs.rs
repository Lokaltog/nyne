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

/// Configurable file names for the analysis plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VfsFiles {
    /// Per-symbol analysis hints file name.
    pub analysis: String,
}

impl Default for VfsFiles {
    fn default() -> Self {
        Self {
            analysis: "ANALYSIS.md".into(),
        }
    }
}
