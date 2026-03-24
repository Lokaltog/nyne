use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::{fs, io};

use color_eyre::eyre::{Result, WrapErr};

use super::file_kind::FileKind;
use super::vfs_path::VfsPath;

/// Metadata for a real filesystem entry.
#[derive(Debug, Clone)]
pub struct FileMeta {
    pub size: u64,
    pub mtime: SystemTime,
    pub file_type: FileKind,
    pub permissions: u32,
}

/// A directory entry returned by [`RealFs::read_dir`].
pub struct DirEntry {
    pub name: String,
    pub file_type: FileKind,
}

/// Abstraction over real filesystem operations.
///
/// Enables testing with mock filesystems while keeping the production
/// implementation thin over `std::fs`.
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
pub struct OsFs {
    source_dir: PathBuf,
}

/// Construction and path resolution helpers for the OS filesystem backend.
impl OsFs {
    pub const fn new(source_dir: PathBuf) -> Self { Self { source_dir } }

    /// Resolve a `VfsPath` to an absolute path on the real filesystem.
    fn resolve(&self, path: &VfsPath) -> PathBuf {
        if path.is_root() {
            self.source_dir.clone()
        } else {
            self.source_dir.join(path.as_str())
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
    fn source_dir(&self) -> &Path { &self.source_dir }

    fn read(&self, path: &VfsPath) -> Result<Vec<u8>> { self.fs_op(path, "read", |p| fs::read(p)) }

    fn write(&self, path: &VfsPath, data: &[u8]) -> Result<()> { self.fs_op(path, "write", |p| fs::write(p, data)) }

    fn exists(&self, path: &VfsPath) -> bool { self.resolve(path).exists() }

    fn is_dir(&self, path: &VfsPath) -> bool { self.resolve(path).is_dir() }

    fn read_dir(&self, path: &VfsPath) -> Result<Vec<DirEntry>> {
        let real_path = self.resolve(path);
        let mut entries = Vec::new();
        let rd = fs::read_dir(&real_path).wrap_err_with(|| format!("failed to read_dir {}", real_path.display()))?;
        for entry in rd {
            let entry = entry.wrap_err("failed to read directory entry")?;
            let file_type = FileKind::from_std(entry.file_type().wrap_err("failed to read file type")?);
            entries.push(DirEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                file_type,
            });
        }
        Ok(entries)
    }

    fn metadata(&self, path: &VfsPath) -> Result<FileMeta> {
        let real_path = self.resolve(path);
        let meta =
            fs::symlink_metadata(&real_path).wrap_err_with(|| format!("failed to stat {}", real_path.display()))?;
        let file_type = FileKind::from_std(meta.file_type());
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

    fn symlink_target(&self, path: &VfsPath) -> Result<PathBuf> { self.fs_op(path, "readlink", |p| fs::read_link(p)) }

    fn rename(&self, from: &VfsPath, to: &VfsPath) -> Result<()> {
        let from_path = self.resolve(from);
        let to_path = self.resolve(to);
        fs::rename(&from_path, &to_path)
            .wrap_err_with(|| format!("failed to rename {} to {}", from_path.display(), to_path.display()))
    }

    fn unlink(&self, path: &VfsPath) -> Result<()> { self.fs_op(path, "unlink", |p| fs::remove_file(p)) }

    fn rmdir(&self, path: &VfsPath) -> Result<()> { self.fs_op(path, "rmdir", |p| fs::remove_dir(p)) }

    fn create_file(&self, path: &VfsPath) -> Result<()> { self.fs_op(path, "create", |p| File::create(p).map(drop)) }

    fn mkdir(&self, path: &VfsPath) -> Result<()> { self.fs_op(path, "mkdir", |p| fs::create_dir_all(p)) }

    fn open_raw(&self, path: &VfsPath) -> Option<File> {
        let real_path = self.resolve(path);
        File::open(&real_path).ok()
    }
}
