//! File kind classification for filesystem entries (real and virtual).
//!
//! Shared discriminant used across all layers — real filesystem metadata,
//! virtual node classification, FUSE attribute generation.

use std::fs::FileType;

use crate::types::NodeKind;

/// Filesystem entry kind: file, directory, or symlink.
///
/// Shared discriminant used across all layers — real filesystem metadata,
/// virtual node classification, FUSE attribute generation, and cache keying.
/// This is intentionally decoupled from `std::fs::FileType` so virtual nodes
/// can produce it without touching the real filesystem.
///
/// Complements [`NodeKind`](crate::types::NodeKind): `NodeKind` is the
/// exhaustive router-level subset the virtual tree can produce, while
/// `FileKind` is `#[non_exhaustive]` so new OS-level types (sockets, FIFOs)
/// can be added without a semver break. Conversion is one-way:
/// `From<NodeKind> for FileKind`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// A regular file (or anything that is neither a directory nor a symlink).
    File,
    /// A directory.
    Directory,
    /// A symbolic link.
    Symlink,
}

impl From<FileType> for FileKind {
    /// Routes through [`NodeKind::from(FileType)`] to keep the
    /// directory/symlink/file classification in a single place.
    fn from(ft: FileType) -> Self { crate::types::NodeKind::from(ft).into() }
}

impl From<NodeKind> for FileKind {
    fn from(nk: NodeKind) -> Self {
        match nk {
            NodeKind::File => Self::File,
            NodeKind::Directory => Self::Directory,
            NodeKind::Symlink => Self::Symlink,
        }
    }
}
