//! Real filesystem backend using `std::fs`.

use std::fs;
use std::fs::File;
use std::io::ErrorKind;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use color_eyre::eyre::Result;

use super::{DirEntry, Filesystem, Metadata};
use crate::router::{AffectedFiles, NodeKind};
use crate::types::Timestamps;

/// Real filesystem backend rooted at a base directory.
///
/// All paths passed to trait methods are interpreted relative to `root`.
pub struct OsFilesystem {
    root: PathBuf,
}

impl OsFilesystem {
    pub fn new(root: impl Into<PathBuf>) -> Self { Self { root: root.into() } }
}

impl Filesystem for OsFilesystem {
    fn source_dir(&self) -> &Path { &self.root }

    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(self.root.join(path))? {
            let entry = entry?;
            entries.push(DirEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                kind: if entry.file_type()?.is_dir() {
                    NodeKind::Directory
                } else {
                    NodeKind::File
                },
            });
        }
        Ok(entries)
    }

    fn stat(&self, dir: &Path, name: &str) -> Result<Option<DirEntry>> {
        match fs::metadata(self.root.join(dir).join(name)) {
            Ok(meta) => Ok(Some(DirEntry {
                name: name.to_owned(),
                kind: if meta.is_dir() {
                    NodeKind::Directory
                } else {
                    NodeKind::File
                },
            })),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn read_file(&self, path: &Path) -> Result<Vec<u8>> { Ok(fs::read(self.root.join(path))?) }

    fn write_file(&self, path: &Path, content: &[u8]) -> Result<AffectedFiles> {
        fs::write(self.root.join(path), content)?;
        Ok(vec![path.to_owned()])
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<()> { Ok(fs::rename(self.root.join(from), self.root.join(to))?) }

    fn remove(&self, path: &Path) -> Result<()> {
        let full = self.root.join(path);
        match fs::remove_file(&full) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::IsADirectory || e.kind() == ErrorKind::PermissionDenied =>
                Ok(fs::remove_dir(&full)?),
            Err(e) => Err(e.into()),
        }
    }

    fn create_file(&self, path: &Path) -> Result<()> {
        File::create(self.root.join(path))?;
        Ok(())
    }

    fn mkdir(&self, path: &Path) -> Result<()> { Ok(fs::create_dir_all(self.root.join(path))?) }

    fn metadata(&self, path: &Path) -> Result<Metadata> {
        let meta = fs::symlink_metadata(self.root.join(path))?;
        let file_type = if meta.is_dir() {
            NodeKind::Directory
        } else if meta.is_symlink() {
            NodeKind::Symlink
        } else {
            NodeKind::File
        };
        Ok(Metadata {
            size: meta.len(),
            timestamps: Timestamps {
                atime: meta.accessed().unwrap_or(UNIX_EPOCH),
                mtime: meta.modified().unwrap_or(UNIX_EPOCH),
                ctime: meta.created().or_else(|_| meta.modified()).unwrap_or(UNIX_EPOCH),
            },
            file_type,
            permissions: meta.mode(),
        })
    }

    fn symlink_target(&self, path: &Path) -> Result<PathBuf> { Ok(fs::read_link(self.root.join(path))?) }
}
