pub mod mem;
pub mod mode;
pub mod os;

use std::path::{Path, PathBuf};

use color_eyre::eyre::Result;

use crate::router::{AffectedFiles, NodeKind};
use crate::types::Timestamps;

/// A directory entry returned by filesystem operations.
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub kind: NodeKind,
}

/// File metadata returned by [`Filesystem::metadata`].
#[derive(Debug, Clone)]
pub struct Metadata {
    /// File size in bytes.
    pub size: u64,
    /// Access, modification, and change times.
    pub timestamps: Timestamps,
    /// Entry kind (file, directory, or symlink).
    pub file_type: NodeKind,
    /// Unix permission bits (e.g., `0o644`).
    pub permissions: u32,
}

/// Backend-agnostic filesystem operations.
///
/// Implementations provide the actual storage layer. The middleware chain
/// dispatches through providers that eventually delegate to this trait
/// for real filesystem operations.
pub trait Filesystem: Send + Sync {
    /// The root directory this filesystem operates on.
    fn source_dir(&self) -> &Path;

    /// List all entries in a directory.
    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>>;

    /// Stat a single entry by name within a directory.
    fn stat(&self, dir: &Path, name: &str) -> Result<Option<DirEntry>>;

    /// Read file contents.
    fn read_file(&self, path: &Path) -> Result<Vec<u8>>;

    /// Write file contents (creates if missing, overwrites if exists).
    ///
    /// Returns the source files affected by the write (for cache invalidation).
    fn write_file(&self, path: &Path, content: &[u8]) -> Result<AffectedFiles>;

    /// Rename/move a file or directory.
    fn rename(&self, from: &Path, to: &Path) -> Result<()>;

    /// Remove a file or directory.
    fn remove(&self, path: &Path) -> Result<()>;

    /// Create an empty file.
    fn create_file(&self, path: &Path) -> Result<()>;

    /// Create a directory.
    fn mkdir(&self, path: &Path) -> Result<()>;

    /// File metadata (size, timestamps, permissions, type). Does not follow symlinks.
    fn metadata(&self, path: &Path) -> Result<Metadata>;

    /// Check whether a path exists.
    fn exists(&self, path: &Path) -> bool { self.metadata(path).is_ok() }

    /// Check whether a path is a directory.
    fn is_dir(&self, path: &Path) -> bool { self.metadata(path).is_ok_and(|m| m.file_type == NodeKind::Directory) }

    /// Read the target of a symbolic link.
    fn symlink_target(&self, path: &Path) -> Result<PathBuf>;
}
