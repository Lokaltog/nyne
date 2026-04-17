//! Virtual filesystem node kind.
//!
//! Router-level subset of filesystem entry types the virtual tree can
//! represent. Intentionally separate from [`FileKind`](crate::types::FileKind),
//! which is `#[non_exhaustive]` and covers OS-level types (sockets, FIFOs)
//! that the router never produces. Conversion is one-way:
//! `From<NodeKind> for FileKind`.

use std::fs::FileType;

/// The kind of virtual filesystem node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    File,
    Directory,
    Symlink,
}

impl From<FileType> for NodeKind {
    /// Anything not a directory or symlink is classified as [`NodeKind::File`],
    /// including special file types like sockets or FIFOs. Mirrors the
    /// [`From<FileType> for FileKind`](crate::types::FileKind) conversion.
    fn from(ft: FileType) -> Self {
        if ft.is_dir() {
            Self::Directory
        } else if ft.is_symlink() {
            Self::Symlink
        } else {
            Self::File
        }
    }
}
