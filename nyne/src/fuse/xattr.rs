//! Extended attribute operations for error reporting and node metadata.

use std::ffi::OsStr;
use std::sync::PoisonError;

use fuser::{Errno, INodeNo, ReplyEmpty, ReplyXattr};
use tracing::{debug, trace};

use super::{FuseFilesystem, extract_errno, fuse_try};

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
            reply.error(Errno::ENODATA);
            return;
        }

        // FUSE-level xattr: last write error.
        if attr_name == XATTR_ERROR {
            let errors = self.write_errors.read().unwrap_or_else(PoisonError::into_inner);
            return match errors.get(&ino) {
                Some(msg) => reply_xattr_data(reply, size, msg.as_bytes()),
                None => reply.error(Errno::ENODATA),
            };
        }

        // Delegate to the node's Attributable capability.
        let Some(entry) = self.inodes.get(ino) else {
            reply.error(Errno::ENOENT);
            return;
        };
        let node = fuse_try!(
            reply,
            self.lookup_node(&entry.dir_path, &entry.name, None),
            ino,
            "getxattr lookup failed"
        );
        let Some(node) = node else {
            reply.error(Errno::ENOENT);
            return;
        };
        let Some(attr) = node.attributable() else {
            reply.error(Errno::ENODATA);
            return;
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
        let mut names = if let Some(entry) = self.inodes.get(ino)
            && let Ok(Some(node)) = self.lookup_node(&entry.dir_path, &entry.name, None)
            && let Some(attr) = node.attributable()
        {
            attr.list()
        } else {
            Vec::new()
        };

        // Include FUSE-level xattr if a write error exists.
        if self
            .write_errors
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(&ino)
        {
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

        let Some(entry) = self.inodes.get(ino) else {
            reply.error(Errno::ENOENT);
            return;
        };
        let node = fuse_try!(
            reply,
            self.lookup_node(&entry.dir_path, &entry.name, None),
            ino,
            "setxattr lookup failed"
        );
        let Some(node) = node else {
            reply.error(Errno::ENOENT);
            return;
        };
        let Some(attr) = node.attributable() else {
            reply.error(Errno::from_i32(libc::ENOTSUP));
            return;
        };
        fuse_try!(reply, attr.set(&attr_name, value), ino, "setxattr failed");
        reply.ok();
    }
}
