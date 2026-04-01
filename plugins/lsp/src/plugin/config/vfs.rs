use nyne::templates::TemplateGlobals;
use serde::{Deserialize, Serialize};

/// VFS path configuration for the LSP plugin.
///
/// Covers the fixed (non-feature-derived) path names. Feature-derived
/// names (e.g. `CALLERS.md`, `callers/`) are determined by the
/// `Feature` slug system in `provider/content/feature.rs`.
///
/// ```toml
/// [plugin.lsp.vfs.dir]
/// actions = "actions"
/// rename = "rename"
/// search = "search"
///
/// [plugin.lsp.vfs.file]
/// diagnostics = "DIAGNOSTICS.md"
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Vfs {
    /// Directory names in the LSP VFS tree.
    pub dir: VfsDirs,

    /// File names in the LSP VFS tree.
    pub file: VfsFiles,
}

impl TemplateGlobals for Vfs {}

/// Configurable directory names for the LSP plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VfsDirs {
    /// Code actions directory inside fragment directories.
    pub actions: String,

    /// Rename directory (companion root for file-level, fragment for symbol-level).
    pub rename: String,

    /// Workspace symbol search directory at companion root.
    pub search: String,
}

impl Default for VfsDirs {
    fn default() -> Self {
        Self {
            actions: "actions".into(),
            rename: "rename".into(),
            search: "search".into(),
        }
    }
}

/// Configurable file names for the LSP plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VfsFiles {
    /// Per-file diagnostics file at companion root.
    pub diagnostics: String,
}

impl Default for VfsFiles {
    fn default() -> Self {
        Self {
            diagnostics: "DIAGNOSTICS.md".into(),
        }
    }
}
