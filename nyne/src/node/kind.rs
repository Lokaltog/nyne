use std::path::PathBuf;

use crate::types::file_kind::FileKind;
use crate::types::vfs_path::VfsPath;

/// The kind of virtual node.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum NodeKind {
    File { size_hint: Option<u64> },
    Directory,
    Symlink { target: PathBuf },
}

impl NodeKind {
    /// Convert to the flat file-kind discriminant.
    ///
    /// Collapses `NodeKind` (which carries data like `size_hint`, symlink `target`)
    /// into the bare `FileKind` enum used by the cache and FUSE layers.
    pub const fn file_kind(&self) -> FileKind {
        match self {
            Self::File { .. } => FileKind::File,
            Self::Directory => FileKind::Directory,
            Self::Symlink { .. } => FileKind::Symlink,
        }
    }
}

/// Optional attribute overrides for a virtual node.
#[derive(Debug, Clone, Default)]
pub struct NodeAttr {
    /// Override the reported file size.
    pub size: Option<u64>,
    /// Custom modification time (seconds since epoch).
    pub mtime: Option<u64>,
    /// Custom creation time (seconds since epoch).
    pub ctime: Option<u64>,
}

/// Outcome of a write operation.
#[non_exhaustive]
#[derive(Debug)]
pub enum WriteOutcome {
    /// Successfully wrote `n` bytes.
    Written(usize),
    /// Write was intentionally ignored.
    Ignored,
    /// Redirect the write to a different path.
    Redirect { path: VfsPath },
}
