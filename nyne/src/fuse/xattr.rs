//! Extended attribute operations for error reporting and metadata.
//!
//! Handles `getxattr`, `listxattr`, and `setxattr` FUSE callbacks. Two layers
//! of xattrs are merged:
//!
//! - **FUSE-level:** `user.error` — stores the last write-pipeline failure message
//!   per inode, enabling `PostToolUse` hooks to surface validation errors to agents.
//!   Managed internally by the flush/release path, not writable via `setxattr`.
//! - **Provider-level:** arbitrary attributes exposed through the node's
//!   [`Xattrable`](crate::node::Xattrable) capability. Providers use these for
//!   node-specific metadata (e.g., staging state for batch edits).
//!
//! The xattr size-query protocol (`size == 0` → return length) is handled by
//! [`reply_xattr_data`], which all three handlers share. Event draining after
//! `setxattr` is owned by the FUSE layer (see `ops.rs` module docs).

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
///
/// Xattrs serve two purposes: FUSE-level metadata (like write error
/// reporting via [`XATTR_ERROR`]) and provider-defined attributes
/// exposed through the [`Xattrable`](crate::node::Xattrable) capability.
impl NyneFs {
    /// Handles getxattr requests, checking FUSE-level attributes first, then
    /// delegating to the node's [`Xattrable`](crate::node::Xattrable) capability.
    ///
    /// The FUSE-level `user.error` attribute is checked before any provider
    /// attributes, ensuring write errors are always accessible even if the
    /// node has no xattr capability.
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

    /// Enumerates available extended attributes for an inode.
    ///
    /// Merges provider-defined xattrs (from the node's [`Xattrable`](crate::node::Xattrable)
    /// capability) with FUSE-level attributes. The `user.error` attribute is
    /// included only when a write error is currently stored for this inode.
    ///
    /// The response is formatted as null-terminated names concatenated into a
    /// single buffer, per the xattr list wire format.
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

    /// Sets an extended attribute, delegating to the node's
    /// [`Xattrable`](crate::node::Xattrable) capability.
    ///
    /// Unlike getxattr, there are no FUSE-level writable attributes —
    /// `user.error` is managed internally by the write pipeline. Returns
    /// `ENOTSUP` for real files and nodes without the xattr capability.
    ///
    /// On success, triggers event processing so providers can react to
    /// attribute changes (e.g., invalidating dependent nodes).
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
