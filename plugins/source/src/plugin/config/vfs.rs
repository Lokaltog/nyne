use serde::{Deserialize, Serialize};

/// VFS path configuration for the source plugin.
///
/// ```toml
/// [plugin.source.vfs.dir]
/// symbols = "symbols"
/// by_kind = "by-kind"
///
/// [plugin.source.vfs.file]
/// body = "body"
/// overview = "OVERVIEW.md"
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Vfs {
    /// Directory names in the source VFS tree.
    pub dir: VfsDirs,

    /// File names in the source VFS tree.
    pub file: VfsFiles,
}

nyne::vfs_struct! {
    /// Configurable directory names for the source plugin.
    pub struct VfsDirs {
        /// Top-level symbol directory inside companion directories.
        symbols = "symbols",
        /// Subdirectory for type-filtered symbol views.
        by_kind = "by-kind",
        /// Subdirectory for line-number-to-symbol lookups.
        at_line = "at-line",
        /// Subdirectory for fenced code blocks.
        code = "code",
        /// Subdirectory for batch edit staging operations.
        edit = "edit",
        /// Subdirectory for previewing staged edit actions.
        staged = "staged",
    }
}

nyne::vfs_struct! {
    /// Configurable file names for the source plugin.
    pub struct VfsFiles {
        /// Overview file in companion and symbol directories.
        overview = "OVERVIEW.md",
        /// Symbol body file base name (extension appended per source language).
        body = "body",
        /// Signature meta-file base name.
        signature = "signature",
        /// Docstring meta-file base name (`.txt` suffix appended at runtime).
        docstring = "docstring",
        /// Decorators/attributes meta-file base name.
        decorators = "decorators",
        /// Imports file base name.
        imports = "imports",
        /// Staged diff file name — apply on delete, clear on truncate.
        staged_diff = "staged.diff",
        /// Per-fragment delete preview file — apply on `rm`.
        delete_diff = "delete.diff",
    }
}
