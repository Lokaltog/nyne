//! FUSE mutation operations — create, mkdir, unlink, rmdir, and rename.
//!
//! These handlers delegate to `Router` mutation methods, which own event
//! draining (see `doc/codebase.md` — "Event Draining"). The FUSE layer
//! does **not** call `process_events()` after these operations — the
//! router handles cache invalidation internally.

use std::ffi::OsStr;

use fuser::{Errno, FileHandle, FopenFlags, INodeNo, ReplyCreate, ReplyEmpty, ReplyEntry, Request};
use tracing::debug;

use super::{GENERATION, NyneFs, extract_errno};

/// FUSE mutation handlers for create, mkdir, remove, and rename.
impl NyneFs {
    /// Handles file creation in a parent directory.
    ///
    /// Asks the router to find a provider willing to claim the create. If
    /// no provider accepts, replies `EACCES`. On success, immediately opens
    /// the new file and returns a handle with `FOPEN_DIRECT_IO` so the
    /// caller can write to it without a separate `open()`.
    pub(super) fn do_create(&self, req: &Request, parent: INodeNo, name: &OsStr, flags: i32, reply: ReplyCreate) {
        with_parent_ctx!(self, parent, name, reply, "create", |parent_ino, name_str, ctx| {
            let result = fuse_try!(reply, self.router.create_node(&name_str, &ctx),
                parent_ino, name = %name_str, "create failed");
            let Some(inode) = result else {
                debug!(target: "nyne::fuse", parent_ino, name = %name_str, "create: no provider claimed");
                reply.error(Errno::EACCES);
                return;
            };
            let content = fuse_try!(reply, self.load_content(inode), inode, "create: failed to load content");
            let fh = self.handles.open(inode, content, flags);
            if let Some((attr, ttl)) = self.build_attr(inode, req) {
                reply.created(&ttl, &attr, GENERATION, FileHandle(fh), FopenFlags::FOPEN_DIRECT_IO);
            } else {
                reply.error(Errno::EIO);
            }
        });
    }

    /// Handles directory creation in a parent directory.
    ///
    /// Delegates to the router's `mkdir_node`, which finds a provider to
    /// claim the directory. Replies `EACCES` if no provider accepts.
    pub(super) fn do_mkdir(&self, req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        with_parent_ctx!(self, parent, name, reply, "mkdir", |parent_ino, name_str, ctx| {
            let result = fuse_try!(reply, self.router.mkdir_node(&name_str, &ctx),
                parent_ino, name = %name_str, "mkdir failed");
            let Some(inode) = result else {
                debug!(target: "nyne::fuse", parent_ino, name = %name_str, "mkdir: no provider claimed");
                reply.error(Errno::EACCES);
                return;
            };
            self.reply_entry(inode, req, reply);
        });
    }

    /// Handles file or directory removal (unlink or rmdir).
    ///
    /// Unified handler for both `unlink` and `rmdir` — the `is_dir` flag
    /// tells the router which semantics to apply. The router decides whether
    /// this is a real filesystem removal or a virtual node unlink based on
    /// the node's [`Unlinkable`](crate::node::capabilities::Unlinkable) capability.
    pub(super) fn do_remove(&self, parent: INodeNo, name: &OsStr, is_dir: bool, reply: ReplyEmpty) {
        let label = if is_dir { "rmdir" } else { "unlink" };
        with_parent_ctx!(self, parent, name, reply, "remove", |parent_ino, name_str, ctx| {
            fuse_try!(reply, self.router.remove_node(&name_str, is_dir, &ctx),
                parent_ino, name = %name_str, label, "remove failed");
            reply.ok();
        });
    }

    /// Handles rename/move of a file or directory.
    ///
    /// Resolves both the source and target parent directories independently
    /// (they may differ for cross-directory moves). The router's
    /// [`rename_node`](crate::dispatch::Router::rename_node) decides whether
    /// the rename targets a real filesystem entry or a virtual node with
    /// [`Renameable`](crate::node::capabilities::Renameable) capability.
    pub(super) fn do_rename(
        &self,
        parent: INodeNo,
        name: &OsStr,
        newparent: INodeNo,
        newname: &OsStr,
        reply: ReplyEmpty,
    ) {
        let parent_ino = u64::from(parent);
        let newparent_ino = u64::from(newparent);
        let name_str = name.to_string_lossy();
        let newname_str = newname.to_string_lossy();
        debug!(target: "nyne::fuse", parent_ino, name = %name_str, newparent_ino, newname = %newname_str, "rename");

        let src_dir = ensure_dir_path!(self, parent_ino, reply);
        let target_dir = ensure_dir_path!(self, newparent_ino, reply);

        let src_ctx = self.router.make_request_context(&src_dir);
        fuse_try!(reply, self.router.rename_node(&name_str, &src_ctx, &target_dir, &newname_str),
            src = %name_str, dst = %newname_str, "rename failed");
        reply.ok();
    }
}
