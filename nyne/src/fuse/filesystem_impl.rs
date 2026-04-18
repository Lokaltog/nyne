//! [`crate::router::Filesystem`] bridge implementation for
//! [`FuseFilesystem`].
//!
//! Callbacks on the router-side `Filesystem` trait delegate through the
//! chain dispatch helpers on [`FuseFilesystem`] so middleware providers
//! see the same request pipeline no matter whether the request arrived
//! via FUSE (`fuser::Filesystem`) or through a programmatic call from
//! another part of the daemon.
//!
//! Split out of `mod.rs` so the bridge is its own named file and the
//! main module can focus on [`FuseFilesystem`]'s inherent API. Only
//! this file has a business reason to import router context types like
//! [`ReadContext`] or [`WriteContext`].

use std::path::{Path, PathBuf};

use color_eyre::eyre::Result;

use super::FuseFilesystem;
use crate::err;
use crate::router::{AffectedFiles, DirEntry, Filesystem, Metadata, Op, ReadContext};

impl Filesystem for FuseFilesystem {
    fn source_dir(&self) -> &Path { self.backing_fs.source_dir() }

    fn metadata(&self, path: &Path) -> Result<Metadata> { self.backing_fs.metadata(path) }

    fn symlink_target(&self, path: &Path) -> Result<PathBuf> { self.backing_fs.symlink_target(path) }

    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        Ok(self
            .read_dir_nodes(path, None)?
            .into_iter()
            .map(|n| DirEntry {
                name: n.name().to_owned(),
                kind: n.kind(),
            })
            .collect())
    }

    fn stat(&self, dir: &Path, name: &str) -> Result<Option<DirEntry>> {
        Ok(self.lookup_node(dir, name, None)?.map(|n| DirEntry {
            name: n.name().to_owned(),
            kind: n.kind(),
        }))
    }

    fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        self.resolve_named(path)?
            .readable()
            .ok_or_else(|| err::not_readable(path))?
            .read(&ReadContext {
                path,
                fs: self.backing_fs.as_ref(),
            })
    }

    fn write_file(&self, path: &Path, content: &[u8]) -> Result<AffectedFiles> {
        self.write_via_node(path, &self.resolve_named(path)?, content)
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<()> { self.dispatch_rename_op(from, to, None) }

    fn remove(&self, path: &Path) -> Result<()> {
        self.dispatch_path_op(path, |name| Op::Remove { name }, None)
            .map(|_| ())
    }

    fn create_file(&self, path: &Path) -> Result<()> {
        self.dispatch_path_op(path, |name| Op::Create { name }, None)
            .map(|_| ())
    }

    fn mkdir(&self, path: &Path) -> Result<()> {
        self.dispatch_path_op(path, |name| Op::Mkdir { name }, None).map(|_| ())
    }
}
