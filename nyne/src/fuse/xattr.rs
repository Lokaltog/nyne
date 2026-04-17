//! Extended attribute operations for error reporting and node metadata.

use std::ffi::OsStr;

use fuser::{Errno, INodeNo, ReplyEmpty, ReplyXattr};
use tracing::{debug, trace};

use super::{FuseFilesystem, fuse_err, fuse_try};
use crate::router::Attributable;

/// Reply with xattr data, respecting the size-query protocol.
///
/// When `size == 0`, returns the data length (size query).
/// When the data exceeds the requested buffer, returns `ERANGE`.
/// Otherwise, returns the data bytes.
pub(super) fn reply_xattr_data(reply: ReplyXattr, size: u32, data: &[u8]) {
    if size == 0 {
        reply.size(u32::try_from(data.len()).unwrap_or(u32::MAX));
    } else if data.len() > size as usize {
        reply.error(Errno::ERANGE);
    } else {
        reply.data(data);
    }
}

/// Extended attribute for the last write error on a file.
///
/// Set by the flush/release path when a write pipeline fails (e.g.
/// tree-sitter validation rejection). Cleared on the next successful
/// flush. Enables `PostToolUse` hooks to surface validation errors.
pub(super) const XATTR_ERROR: &str = "user.error";

impl FuseFilesystem {
    pub(super) fn do_getxattr(&self, ino: INodeNo, name: &OsStr, size: u32, reply: ReplyXattr) {
        let ino = u64::from(ino);
        let attr_name = name.to_string_lossy();
        trace!(target: "nyne::fuse", ino, name = %attr_name, "getxattr");

        // Only the `user.*` namespace is relevant for virtual attributes.
        // Short-circuit `security.*`, `system.*`, `trusted.*` etc. — the
        // kernel queries these on every file access and dispatching through
        // the chain for each one is pure overhead.
        if !attr_name.starts_with("user.") {
            fuse_err!(reply, Errno::ENODATA, ino, "getxattr: non-user namespace");
        }

        // FUSE-level xattr: last write error.
        if attr_name == XATTR_ERROR {
            return match self.inode_state.write_error(ino) {
                Some(msg) => reply_xattr_data(reply, size, msg.as_bytes()),
                None => reply.error(Errno::ENODATA),
            };
        }

        // Delegate to the node's Attributable capability.
        let Some(node) = fuse_try!(reply, self.resolve_node_for_inode(ino), ino, "getxattr lookup failed") else {
            fuse_err!(reply, Errno::ENOENT, ino, "getxattr: node not found");
        };
        let Some(attr) = node.attributable() else {
            fuse_err!(reply, Errno::ENODATA, ino, "getxattr: attributable unsupported");
        };
        match attr.get(&attr_name) {
            Some(data) => reply_xattr_data(reply, size, &data),
            None => reply.error(Errno::ENODATA),
        }
    }

    pub(super) fn do_listxattr(&self, ino: INodeNo, size: u32, reply: ReplyXattr) {
        let ino = u64::from(ino);
        trace!(target: "nyne::fuse", ino, "listxattr");

        // Collect names from the node's Attributable capability.
        let mut names = self
            .resolve_node_for_inode(ino)
            .ok()
            .flatten()
            .and_then(|n| n.attributable().map(Attributable::list))
            .unwrap_or_default();

        // Include FUSE-level xattr if a write error exists.
        if self.inode_state.has_write_error(ino) {
            names.push(XATTR_ERROR.to_owned());
        }

        // xattr list format: null-terminated names concatenated.
        let mut buf = Vec::new();
        for name in &names {
            buf.extend_from_slice(name.as_bytes());
            buf.push(0);
        }
        reply_xattr_data(reply, size, &buf);
    }

    pub(super) fn do_setxattr(&self, ino: INodeNo, name: &OsStr, value: &[u8], reply: ReplyEmpty) {
        let ino = u64::from(ino);
        let attr_name = name.to_string_lossy();
        debug!(target: "nyne::fuse", ino, name = %attr_name, "setxattr");

        let Some(node) = fuse_try!(reply, self.resolve_node_for_inode(ino), ino, "setxattr lookup failed") else {
            fuse_err!(reply, Errno::ENOENT, ino, "setxattr: node not found");
        };
        let Some(attr) = node.attributable() else {
            fuse_err!(
                reply,
                Errno::from_i32(libc::ENOTSUP),
                ino,
                "setxattr: attributable unsupported"
            );
        };
        fuse_try!(reply, attr.set(&attr_name, value), ino, "setxattr failed");
        reply.ok();
    }
}
