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

/// Configurable directory names for the source plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VfsDirs {
    /// Top-level symbol directory inside companion directories.
    pub symbols: String,

    /// Subdirectory for type-filtered symbol views.
    pub by_kind: String,

    /// Subdirectory for line-number-to-symbol lookups.
    pub at_line: String,

    /// Subdirectory for fenced code blocks.
    pub code: String,

    /// Subdirectory for batch edit staging operations.
    pub edit: String,

    /// Subdirectory for previewing staged edit actions.
    pub staged: String,
}

impl Default for VfsDirs {
    fn default() -> Self {
        Self {
            symbols: "symbols".into(),
            by_kind: "by-kind".into(),
            at_line: "at-line".into(),
            code: "code".into(),
            edit: "edit".into(),
            staged: "staged".into(),
        }
    }
}

/// Configurable file names for the source plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VfsFiles {
    /// Overview file in companion and symbol directories.
    pub overview: String,

    /// Symbol body file base name (extension appended per source language).
    pub body: String,

    /// Signature meta-file base name.
    pub signature: String,

    /// Docstring meta-file base name (`.txt` suffix appended at runtime).
    pub docstring: String,

    /// Decorators/attributes meta-file base name.
    pub decorators: String,

    /// Imports file base name.
    pub imports: String,

    /// Staged diff file name — apply on delete, clear on truncate.
    pub staged_diff: String,
}

impl Default for VfsFiles {
    fn default() -> Self {
        Self {
            overview: "OVERVIEW.md".into(),
            body: "body".into(),
            signature: "signature".into(),
            docstring: "docstring".into(),
            decorators: "decorators".into(),
            imports: "imports".into(),
            staged_diff: "staged.diff".into(),
        }
    }
}
