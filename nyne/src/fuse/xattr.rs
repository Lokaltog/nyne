//! Extended attribute operations for error reporting and metadata.

use std::ffi::OsStr;

use fuser::{Errno, INodeNo, ReplyEmpty, ReplyXattr};
use tracing::{debug, trace};

use super::{NyneFs, extract_errno};

/// Reply with xattr data, respecting the size-query protocol.
///
/// When `size == 0`, returns the data length (size query).
/// When the data exceeds the requested buffer, returns `ERANGE`.
/// Otherwise, returns the data bytes.
fn reply_xattr_data(reply: ReplyXattr, size: u32, data: &[u8]) {
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
const XATTR_ERROR: &str = "user.error";

/// Extended attribute handlers for the FUSE filesystem.
impl NyneFs {
    /// Handles getxattr, returning error messages or provider-defined attributes.
    pub(super) fn do_getxattr(&self, ino: INodeNo, name: &OsStr, size: u32, reply: ReplyXattr) {
        let ino = u64::from(ino);
        let name_str = name.to_string_lossy();
        trace!(target: "nyne::fuse", ino, name = %name_str, "getxattr");

        // FUSE-level xattr: last write error.
        if name_str == XATTR_ERROR {
            let errors = self.write_errors.read();
            return match errors.get(&ino) {
                Some(msg) => reply_xattr_data(reply, size, msg.as_bytes()),
                None => reply.error(Errno::ENODATA),
            };
        }

        let result = self.with_inode_io(
            ino,
            |_path| Ok(None), // Real files: no virtual xattrs
            |node, _provider, ctx| {
                let Some(xattr) = node.xattrable() else {
                    return Ok(None);
                };
                xattr.get_xattr(ctx, &name_str)
            },
        );

        let result = fuse_try!(reply, result, ino, name = %name_str, "getxattr failed");
        match result {
            Some(data) => reply_xattr_data(reply, size, &data),
            None => reply.error(Errno::ENODATA),
        }
    }

    /// Handles listxattr, enumerating available extended attributes.
    pub(super) fn do_listxattr(&self, ino: INodeNo, size: u32, reply: ReplyXattr) {
        let ino = u64::from(ino);
        trace!(target: "nyne::fuse", ino, "listxattr");

        let mut names: Vec<String> = fuse_try!(
            reply,
            self.with_inode_io(
                ino,
                |_path| Ok(Vec::new()), // Real files: no virtual xattrs
                |node, _provider, ctx| {
                    let Some(xattr) = node.xattrable() else {
                        return Ok(Vec::new());
                    };
                    Ok(xattr.list_xattrs(ctx))
                },
            ),
            ino,
            "listxattr failed"
        );

        // Include FUSE-level xattr if a write error exists for this inode.
        if self.write_errors.read().contains_key(&ino) {
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

    /// Handles setxattr, delegating to the node's xattr capability.
    pub(super) fn do_setxattr(&self, ino: INodeNo, name: &OsStr, value: &[u8], reply: ReplyEmpty) {
        let ino = u64::from(ino);
        let name_str = name.to_string_lossy();
        debug!(target: "nyne::fuse", ino, name = %name_str, "setxattr");

        let result = self.with_inode_io(
            ino,
            |_path| {
                // Real files: not supported through virtual layer.
                Ok(false)
            },
            |node, _provider, ctx| {
                let Some(xattr) = node.xattrable() else {
                    // No capability: signal via Ok(false).
                    return Ok(false);
                };
                xattr.set_xattr(ctx, &name_str, value)?;
                Ok(true)
            },
        );

        let result = fuse_try!(reply, result, ino, name = %name_str, "setxattr failed");
        if result {
            self.router.process_events();
            reply.ok();
        } else {
            reply.error(Errno::ENOTSUP);
        }
    }
}
