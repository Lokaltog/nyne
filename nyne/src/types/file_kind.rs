//! File kind classification for filesystem entries (real and virtual).
//!
//! Shared discriminant used across all layers -- cache, dispatch, FUSE.
//! NOT coupled to the real filesystem; virtual nodes produce this via
//! [`NodeKind::file_kind()`](crate::node::NodeKind::file_kind).

use std::fs::FileType;

/// Filesystem entry kind: file, directory, or symlink.
///
/// Shared discriminant used across all layers -- real filesystem metadata,
/// virtual node classification, FUSE attribute generation, and cache keying.
/// This is intentionally decoupled from `std::fs::FileType` so virtual nodes
/// can produce it without touching the real filesystem.
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

/// Converts a [`std::fs::FileType`] into a [`FileKind`].
///
/// Anything that is not a directory or symlink is classified as [`FileKind::File`],
/// including special file types like sockets or FIFOs. This matches the FUSE
/// layer's needs where only these three kinds are relevant for attribute generation.
impl From<FileType> for FileKind {
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
