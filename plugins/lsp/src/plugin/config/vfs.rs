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

nyne::vfs_struct! {
    /// Configurable directory names for the LSP plugin.
    pub struct VfsDirs {
        /// Code actions directory inside fragment directories.
        actions = "actions",
        /// Rename directory (companion root for file-level, fragment for symbol-level).
        rename = "rename",
        /// Workspace symbol search directory at companion root.
        search = "search",
    }
}

nyne::vfs_struct! {
    /// Configurable file names for the LSP plugin.
    pub struct VfsFiles {
        /// Per-file diagnostics file at companion root.
        diagnostics = "DIAGNOSTICS.md",
    }
}
