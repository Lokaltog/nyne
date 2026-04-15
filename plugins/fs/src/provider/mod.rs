use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use color_eyre::eyre::Result;
use nyne::router::{
    AffectedFiles, DirEntry, Filesystem, NamedNode, Next, Node, NodeKind, Op, Provider, ReadContext, Readable, Request,
    Writable, WriteContext,
};
use nyne_companion::CompanionRequest;

/// Terminal filesystem provider. Delegates all operations to a `Filesystem` backend.
pub struct FsProvider {
    pub(crate) fs: Arc<dyn Filesystem>,
}

nyne::define_provider!(FsProvider, "fs", terminal: true);

impl Provider for FsProvider {
    fn accept(&self, req: &mut Request, _next: &Next) -> Result<()> {
        if req.companion().is_some() {
            // Read ops: no real-fs contribution for companion paths, not an error.
            // Mutation ops: if no upstream middleware handled it, reject — the
            // companion namespace is a closed world and unhandled mutations must
            // not silently succeed.
            return match req.op() {
                Op::Readdir | Op::Lookup { .. } => Ok(()),
                _ => Err(io::Error::from(io::ErrorKind::PermissionDenied).into()),
            };
        }
        match req.op().clone() {
            Op::Readdir => {
                let Ok(entries) = self.fs.read_dir(req.path()) else {
                    return Ok(());
                };
                for entry in entries {
                    req.nodes.add(build_node(&self.fs, req.path(), &entry));
                }
            }
            Op::Lookup { name } =>
                if let Some(entry) = self.fs.stat(req.path(), &name)? {
                    req.nodes.add(build_node(&self.fs, req.path(), &entry));
                },
            Op::Rename {
                src_name,
                target_dir,
                target_name,
            } => {
                self.fs
                    .rename(&req.path().join(&src_name), &target_dir.join(&target_name))?;
            }
            Op::Remove { name } => {
                self.fs.remove(&req.path().join(&name))?;
            }
            Op::Create { name } => {
                self.fs.create_file(&req.path().join(&name))?;
            }
            Op::Mkdir { name } => {
                self.fs.mkdir(&req.path().join(&name))?;
            }
        }
        Ok(())
    }
}

/// Readable capability backed by a `Filesystem`.
struct FsReadable {
    fs: Arc<dyn Filesystem>,
    path: PathBuf,
}

impl Readable for FsReadable {
    fn read(&self, _ctx: &ReadContext<'_>) -> Result<Vec<u8>> { self.fs.read_file(&self.path) }

    fn backing_path(&self) -> Option<&Path> { Some(&self.path) }
}

/// Writable capability backed by a `Filesystem`.
struct FsWritable {
    fs: Arc<dyn Filesystem>,
    path: PathBuf,
}

impl Writable for FsWritable {
    fn write(&self, _ctx: &WriteContext<'_>, content: &[u8]) -> Result<AffectedFiles> {
        self.fs.write_file(&self.path, content)
    }
}

fn build_node(fs: &Arc<dyn Filesystem>, dir: &Path, entry: &DirEntry) -> NamedNode {
    let file_path = dir.join(&entry.name);
    match entry.kind {
        NodeKind::File => Node::file()
            .with_readable(FsReadable {
                fs: Arc::clone(fs),
                path: file_path.clone(),
            })
            .with_writable(FsWritable {
                fs: Arc::clone(fs),
                path: file_path,
            }),
        // Directories get a readable for backing_path so the FUSE layer
        // can read real filesystem metadata (permissions, mtime).
        NodeKind::Directory => Node::dir().with_readable(FsReadable {
            fs: Arc::clone(fs),
            path: file_path,
        }),
        NodeKind::Symlink => Node::symlink(fs.symlink_target(&file_path).unwrap_or_else(|e| {
            tracing::warn!(
                path = %file_path.display(),
                error = %e,
                "failed to read symlink target; using empty target",
            );
            PathBuf::new()
        })),
    }
    .named(&entry.name)
}
