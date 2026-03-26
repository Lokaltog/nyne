//! Node kind classification and write outcome types.
//!
//! These types capture the "what kind of thing is this" aspect of a node,
//! separate from its behavioral capabilities. [`NodeKind`] determines FUSE
//! file type, default permissions, and `getattr` responses. [`WriteOutcome`]
//! encodes the three possible results of a write operation.

use std::path::PathBuf;

use crate::types::file_kind::FileKind;
use crate::types::vfs_path::VfsPath;

/// The kind of virtual node, determining its FUSE file type and `getattr` shape.
///
/// Marked `#[non_exhaustive]` to allow future kinds (e.g., FIFO, socket)
/// without a semver break.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum NodeKind {
    /// A regular file, optionally with a pre-declared size hint for `getattr`.
    ///
    /// The `size_hint` is advisory — it helps tools that pre-allocate buffers
    /// based on reported size, but the actual content length may differ.
    File { size_hint: Option<u64> },
    /// A directory that can contain child nodes.
    Directory,
    /// A symbolic link pointing to `target`.
    Symlink { target: PathBuf },
}

/// Methods for converting between node kind representations.
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

/// Optional attribute overrides returned by [`Lifecycle::getattr`](super::Lifecycle::getattr).
///
/// When a node's lifecycle hook returns `Some(NodeAttr)`, the FUSE layer
/// merges these overrides into the standard `getattr` response. Fields
/// left as `None` fall back to the defaults (size from content length,
/// mtime/ctime from mount time).
#[derive(Debug, Clone, Default)]
pub struct NodeAttr {
    /// Override the reported file size.
    pub size: Option<u64>,
    /// Custom modification time (seconds since epoch).
    pub mtime: Option<u64>,
    /// Custom creation time (seconds since epoch).
    pub ctime: Option<u64>,
}

/// Outcome of a [`Writable::write`](super::Writable::write) operation.
///
/// Returned by writable implementations to tell the FUSE pipeline how
/// to report the result back to the calling process. Marked
/// `#[non_exhaustive]` for future extension.
#[non_exhaustive]
#[derive(Debug)]
pub enum WriteOutcome {
    /// Successfully wrote `n` bytes — reported as the write size to FUSE.
    Written(usize),
    /// Write was intentionally ignored (e.g., duplicate or no-op content).
    ///
    /// The FUSE layer reports success with 0 bytes written.
    Ignored,
    /// Redirect the write to a different VFS path.
    ///
    /// The dispatch layer re-resolves the target path and replays the write
    /// there. Used by nodes that act as write proxies (e.g., a staging
    /// directory that forwards to the canonical location).
    Redirect { path: VfsPath },
}
