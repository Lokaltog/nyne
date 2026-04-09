//! FUSE mutation operations — create, mkdir, unlink, rmdir, and rename.
//!
//! All mutations dispatch through the middleware chain via
//! [`crate::router::Filesystem`] methods on [`FuseFilesystem`].

use std::ffi::OsStr;
use std::sync::Arc;

use fuser::{Errno, FileHandle, FopenFlags, INodeNo, ReplyCreate, ReplyEmpty, ReplyEntry, Request};
use tracing::debug;

use super::attrs::GENERATION;
use super::{FuseFilesystem, ensure_dir_path, extract_errno, fuse_try};
use crate::prelude::Op;

impl FuseFilesystem {
    /// Handle file creation in a parent directory.
    ///
    /// Dispatches `Op::Create` through the chain, then opens the new file
    /// and returns a handle with `FOPEN_DIRECT_IO`.
    pub(super) fn do_create(&self, req: &Request, parent: INodeNo, name: &OsStr, flags: i32, reply: ReplyCreate) {
        let parent = u64::from(parent);
        let dir_path = ensure_dir_path!(self, parent, reply);
        let name = name.to_string_lossy();
        let process = self.process_from(req);
        debug!(target: "nyne::fuse", parent, name = %name, "create");

        if !self.is_writable_dir(parent, req) {
            reply.error(Errno::EACCES);
            return;
        }

        let path = dir_path.join(name.as_ref());
        fuse_try!(
            reply,
            self.dispatch_path_op(&path, |name| Op::Create { name }, Some(process.clone())),
            parent,
            "create failed"
        );

        match self.resolve_inode(&dir_path, &name, parent, Some(process)) {
            Ok(Some((ino, node))) => {
                let fh = self.handles.open(ino, Arc::from([]), flags);
                let (attr, ttl) = self.node_attr(ino, &node, req);
                reply.created(&ttl, &attr, GENERATION, FileHandle(fh), FopenFlags::FOPEN_DIRECT_IO);
            }
            Ok(None) | Err(_) => reply.error(Errno::EIO),
        }
    }

    /// Handle directory creation in a parent directory.
    pub(super) fn do_mkdir(&self, req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        let parent = u64::from(parent);
        let dir_path = ensure_dir_path!(self, parent, reply);
        let name = name.to_string_lossy();
        let process = self.process_from(req);
        debug!(target: "nyne::fuse", parent, name = %name, "mkdir");

        if !self.is_writable_dir(parent, req) {
            reply.error(Errno::EACCES);
            return;
        }

        let path = dir_path.join(name.as_ref());
        fuse_try!(
            reply,
            self.dispatch_path_op(&path, |name| Op::Mkdir { name }, Some(process.clone())),
            parent,
            "mkdir failed"
        );

        match self.resolve_inode(&dir_path, &name, parent, Some(process)) {
            Ok(Some((ino, node))) => {
                let (attr, ttl) = self.node_attr(ino, &node, req);
                reply.entry(&ttl, &attr, GENERATION);
            }
            Ok(None) => reply.error(Errno::ENOENT),
            Err(e) => reply.error(extract_errno(&e)),
        }
    }

    /// Handle file or directory removal (unlink or rmdir).
    ///
    /// Tries the node's [`Unlinkable`] capability first. Falls back to
    /// chain dispatch so middleware providers (e.g. the diff plugin) can
    /// handle remove operations for virtual nodes.
    pub(super) fn do_remove(&self, req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEmpty) {
        let parent = u64::from(parent);
        let dir_path = ensure_dir_path!(self, parent, reply);
        let name = name.to_string_lossy();
        debug!(target: "nyne::fuse", parent, name = %name, "remove");

        let path = dir_path.join(name.as_ref());

        // Try node capability first (e.g. rmdir on a symbol directory).
        match self.remove_node(&path) {
            Ok(true) => {
                reply.ok();
                return;
            }
            Err(e) => {
                fuse_try!(reply, Err::<(), _>(e), parent, "remove failed");
            }
            Ok(false) => {}
        }

        // Fall back to chain dispatch — middleware providers handle virtual removes.
        let process = Some(self.process_from(req));
        fuse_try!(
            reply,
            self.dispatch_path_op(&path, |name| Op::Remove { name }, process),
            parent,
            "remove failed"
        );
        reply.ok();
    }

    /// Handle rename/move of a file or directory.
    ///
    /// Tries the node's [`Renameable`] capability first. Falls back to
    /// `is_writable_dir` + chain dispatch for real filesystem paths.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn do_rename(
        &self,
        req: &Request,
        parent: INodeNo,
        name: &OsStr,
        newparent: INodeNo,
        newname: &OsStr,
        reply: ReplyEmpty,
    ) {
        let parent = u64::from(parent);
        let newparent_ino = u64::from(newparent);
        let name = name.to_string_lossy();
        let newname = newname.to_string_lossy();
        let process = self.process_from(req);
        debug!(target: "nyne::fuse", parent, name = %name, newparent_ino, newname = %newname, "rename");

        let src_dir = ensure_dir_path!(self, parent, reply);
        let dst_dir = ensure_dir_path!(self, newparent_ino, reply);

        // When `mv` (without `-T`) targets a directory, the kernel probes the
        // destination name via lookup. If the VFS resolves it as a directory
        // (companion `@` dirs always do), `mv` issues a nest-into rename:
        //   rename(parent=symbols/, "Foo@", newparent=Bar@/, "Foo@")
        // instead of the intended same-level rename. Detect this pattern —
        // different parents, same entry name, target is a direct child of
        // source's parent — and rewrite to the intended rename.
        let dst = if parent != newparent_ino && name == newname && dst_dir.starts_with(&src_dir) {
            src_dir.join(dst_dir.file_name().unwrap_or_else(|| newname.as_ref().as_ref()))
        } else {
            dst_dir.join(newname.as_ref())
        };
        let src = src_dir.join(name.as_ref());

        // Try node capability first (e.g. LSP symbol rename).
        match self.rename_node(&src, &dst) {
            Ok(true) => {
                if let Some(ino) = self.inodes.find_inode(&src_dir, &name) {
                    self.inodes.update(ino, dst_dir, newname.into_owned(), newparent_ino);
                }
                reply.ok();
                return;
            }
            Err(e) => {
                fuse_try!(reply, Err::<(), _>(e), parent, "rename failed");
            }
            Ok(false) => {}
        }

        // Fall back to chain dispatch (real filesystem paths).
        if !self.is_writable_dir(parent, req) {
            reply.error(Errno::EACCES);
            return;
        }
        fuse_try!(
            reply,
            self.dispatch_rename_op(&src, &dst, Some(process)),
            parent,
            "rename failed"
        );

        if let Some(ino) = self.inodes.find_inode(&src_dir, &name) {
            self.inodes.update(ino, dst_dir, newname.into_owned(), newparent_ino);
        }
        reply.ok();
    }
}
