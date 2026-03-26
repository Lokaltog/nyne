//! Abstraction over real filesystem operations for FUSE passthrough.
//!
//! The [`RealFs`] trait decouples the dispatch layer from `std::fs`, enabling
//! mock filesystems in tests while keeping the production [`OsFs`]
//! implementation a thin delegation to the standard library. All paths are
//! expressed as [`VfsPath`]s; the implementation resolves them against a
//! root directory (the overlay merged view in production).

use std::borrow::Cow;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::{fs, io};

use color_eyre::eyre::{Result, WrapErr};

use super::file_kind::FileKind;
use super::vfs_path::VfsPath;

/// Metadata for a real filesystem entry.
///
/// Mirrors the subset of `std::fs::Metadata` that the FUSE layer needs for
/// generating file attributes. Retrieved via [`RealFs::metadata`], which uses
/// `symlink_metadata` (no symlink following) to match FUSE `lstat` semantics.
#[derive(Debug, Clone)]
pub struct FileMeta {
    /// File size in bytes.
    pub size: u64,
    /// Last modification time, used for FUSE `mtime` attributes and
    /// cache staleness detection.
    pub mtime: SystemTime,
    /// Entry kind (file, directory, or symlink).
    pub file_type: FileKind,
    /// Unix permission bits (e.g., `0o644`).
    pub permissions: u32,
}

/// A directory entry returned by [`RealFs::read_dir`].
///
/// Contains only the entry name and its kind -- no full path or metadata.
/// The dispatch layer uses these to build the real-file portion of directory
/// listings before merging virtual nodes from providers.
#[derive(Debug)]
pub struct DirEntry {
    /// The entry's filename (no path prefix).
    pub name: String,
    /// Whether this entry is a file, directory, or symlink.
    pub file_type: FileKind,
}

/// Abstraction over real filesystem operations.
///
/// All FUSE daemon I/O against the project directory goes through this trait.
/// The production implementation ([`OsFs`]) delegates to `std::fs`, while
/// tests inject stubs that never touch the disk. This is the only way the
/// dispatch layer reaches the real filesystem -- there are no direct `std::fs`
/// calls in the provider or routing code.
pub trait RealFs: Send + Sync {
    /// The overlay merged directory path.
    ///
    /// Returns the overlay merged view path where the daemon performs all I/O.
    /// This is a separate mount point from the FUSE overlay — no re-entrancy.
    fn source_dir(&self) -> &Path;

    /// Read the entire contents of a file.
    fn read(&self, path: &VfsPath) -> Result<Vec<u8>>;

    /// Write data to a file, creating or overwriting it.
    fn write(&self, path: &VfsPath, data: &[u8]) -> Result<()>;

    /// Check whether a path exists.
    fn exists(&self, path: &VfsPath) -> bool;

    /// Check whether a path is a directory.
    fn is_dir(&self, path: &VfsPath) -> bool;

    /// List entries in a directory.
    fn read_dir(&self, path: &VfsPath) -> Result<Vec<DirEntry>>;

    /// Get metadata for a path (uses `symlink_metadata` — does not follow symlinks).
    fn metadata(&self, path: &VfsPath) -> Result<FileMeta>;

    /// Read the target of a symbolic link.
    fn symlink_target(&self, path: &VfsPath) -> Result<PathBuf>;

    /// Rename a file or directory.
    fn rename(&self, from: &VfsPath, to: &VfsPath) -> Result<()>;

    /// Delete a file.
    fn unlink(&self, path: &VfsPath) -> Result<()>;

    /// Remove an empty directory.
    fn rmdir(&self, path: &VfsPath) -> Result<()>;

    /// Create an empty file.
    fn create_file(&self, path: &VfsPath) -> Result<()>;

    /// Create a directory.
    fn mkdir(&self, path: &VfsPath) -> Result<()>;

    /// Open a raw file handle for FUSE kernel passthrough.
    ///
    /// Returns `None` if the implementation doesn't support raw handles
    /// (e.g., mock/test filesystems). The FUSE handler falls back to
    /// buffered I/O when this returns `None`.
    fn open_raw(&self, _path: &VfsPath) -> Option<File> { None }
}

/// Production [`RealFs`] implementation backed by `std::fs`.
///
/// All operations are rooted at `source_dir`, which points to the overlay
/// merged view in production. [`VfsPath`]s are resolved by joining them
/// onto this root. Errors are wrapped with the verb and resolved path
/// for clear diagnostics (e.g., "failed to read /overlay/merged/src/main.rs").
pub struct OsFs {
    source_dir: PathBuf,
}

/// Construction and path resolution helpers for the OS filesystem backend.
impl OsFs {
    /// Creates a new OS filesystem rooted at the given directory.
    pub const fn new(source_dir: PathBuf) -> Self { Self { source_dir } }

    /// Resolve a `VfsPath` to an absolute path on the real filesystem.
    fn resolve(&self, path: &VfsPath) -> Cow<'_, Path> {
        if path.is_root() {
            Cow::Borrowed(&self.source_dir)
        } else {
            Cow::Owned(self.source_dir.join(path.as_str()))
        }
    }

    /// Resolve a path and run an `io::Result`-returning operation, wrapping
    /// any error with the verb and resolved path for context.
    fn fs_op<T>(&self, path: &VfsPath, verb: &str, f: impl FnOnce(&Path) -> io::Result<T>) -> Result<T> {
        let real = self.resolve(path);
        f(&real).wrap_err_with(|| format!("failed to {verb} {}", real.display()))
    }
}

/// [`RealFs`] implementation backed by `std::fs` operations.
impl RealFs for OsFs {
    /// Returns the root directory this filesystem operates on.
    fn source_dir(&self) -> &Path { &self.source_dir }

    /// Reads the entire contents of a file into a byte vector.
    fn read(&self, path: &VfsPath) -> Result<Vec<u8>> { self.fs_op(path, "read", |p| fs::read(p)) }

    /// Writes data to a file, creating or overwriting it.
    fn write(&self, path: &VfsPath, data: &[u8]) -> Result<()> { self.fs_op(path, "write", |p| fs::write(p, data)) }

    /// Returns whether the path exists on disk.
    fn exists(&self, path: &VfsPath) -> bool { self.resolve(path).exists() }

    /// Returns whether the path is a directory.
    fn is_dir(&self, path: &VfsPath) -> bool { self.resolve(path).is_dir() }

    /// Lists directory entries with their file types.
    fn read_dir(&self, path: &VfsPath) -> Result<Vec<DirEntry>> {
        let real_path = self.resolve(path);
        let mut entries = Vec::new();
        let rd = fs::read_dir(&real_path).wrap_err_with(|| format!("failed to read_dir {}", real_path.display()))?;
        for entry in rd {
            let entry = entry.wrap_err("failed to read directory entry")?;
            let file_type = FileKind::from(entry.file_type().wrap_err("failed to read file type")?);
            entries.push(DirEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                file_type,
            });
        }
        Ok(entries)
    }

    /// Returns file metadata (size, mtime, type, permissions) without following symlinks.
    fn metadata(&self, path: &VfsPath) -> Result<FileMeta> {
        let real_path = self.resolve(path);
        let meta =
            fs::symlink_metadata(&real_path).wrap_err_with(|| format!("failed to stat {}", real_path.display()))?;
        let file_type = FileKind::from(meta.file_type());
        Ok(FileMeta {
            size: meta.len(),
            mtime: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            file_type,
            permissions: {
                use std::os::unix::fs::PermissionsExt;
                meta.permissions().mode()
            },
        })
    }

    /// Reads the target of a symlink.
    fn symlink_target(&self, path: &VfsPath) -> Result<PathBuf> { self.fs_op(path, "readlink", |p| fs::read_link(p)) }

    /// Renames a file or directory from one path to another.
    fn rename(&self, from: &VfsPath, to: &VfsPath) -> Result<()> {
        let from_path = self.resolve(from);
        let to_path = self.resolve(to);
        fs::rename(&from_path, &to_path)
            .wrap_err_with(|| format!("failed to rename {} to {}", from_path.display(), to_path.display()))
    }

    /// Removes a file.
    fn unlink(&self, path: &VfsPath) -> Result<()> { self.fs_op(path, "unlink", |p| fs::remove_file(p)) }

    /// Removes an empty directory.
    fn rmdir(&self, path: &VfsPath) -> Result<()> { self.fs_op(path, "rmdir", |p| fs::remove_dir(p)) }

    /// Creates an empty file, truncating if it already exists.
    fn create_file(&self, path: &VfsPath) -> Result<()> { self.fs_op(path, "create", |p| File::create(p).map(drop)) }

    /// Creates a directory and all missing parents.
    fn mkdir(&self, path: &VfsPath) -> Result<()> { self.fs_op(path, "mkdir", |p| fs::create_dir_all(p)) }

    /// Opens a file for reading, returning `None` if it does not exist.
    fn open_raw(&self, path: &VfsPath) -> Option<File> {
        let real_path = self.resolve(path);
        File::open(&real_path).ok()
    }
}
