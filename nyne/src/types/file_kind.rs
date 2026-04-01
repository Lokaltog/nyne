//! File kind classification for filesystem entries (real and virtual).
//!
//! Shared discriminant used across all layers — real filesystem metadata,
//! virtual node classification, FUSE attribute generation.

use std::fs::FileType;

use crate::router::NodeKind;

/// Filesystem entry kind: file, directory, or symlink.
///
/// Shared discriminant used across all layers — real filesystem metadata,
/// virtual node classification, FUSE attribute generation, and cache keying.
/// This is intentionally decoupled from `std::fs::FileType` so virtual nodes
/// can produce it without touching the real filesystem.
///
/// The router layer has its own [`NodeKind`](crate::router::NodeKind) — the
/// subset of entry types the virtual tree supports. The two enums are
/// intentionally separate: `FileKind` is the Tier-0 cross-layer discriminant
/// (and `#[non_exhaustive]` to allow future OS-level types like sockets),
/// while `NodeKind` is a Tier-1 router concept. Conversion is one-way:
/// `From<NodeKind> for FileKind`.
///
/// Marked `#[non_exhaustive]` to allow future additions (e.g., sockets,
/// FIFOs) without a semver break.
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
    /// Anything not a directory or symlink is classified as [`FileKind::File`],
    /// including special file types like sockets or FIFOs.
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

impl From<NodeKind> for FileKind {
    fn from(nk: NodeKind) -> Self {
        match nk {
            NodeKind::File => Self::File,
            NodeKind::Directory => Self::Directory,
            NodeKind::Symlink => Self::Symlink,
        }
    }
}
