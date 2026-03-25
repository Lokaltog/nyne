/// File kind classification for filesystem entries (real and virtual).
///
/// Shared discriminant used across all layers — cache, dispatch, FUSE.
/// NOT coupled to the real filesystem; virtual nodes produce this via
/// [`NodeKind::file_kind()`](crate::node::NodeKind::file_kind).
use std::fs::FileType;

/// Filesystem entry kind: file, directory, or symlink.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    File,
    Directory,
    Symlink,
}

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
