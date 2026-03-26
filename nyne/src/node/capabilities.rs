//! Capability traits that define what operations a [`VirtualNode`](super::VirtualNode) supports.
//!
//! Nodes compose behavior by optionally attaching trait objects for each
//! capability. The FUSE layer checks for the presence of each capability
//! before dispatching an operation — a node without [`Writable`] will
//! reject writes with `PermissionDenied`, and permissions are auto-derived
//! from which capabilities are attached.
//!
//! All capability traits require `Send + Sync` because nodes are shared
//! across FUSE worker threads via `Arc<VirtualNode>`.

use super::kind::{NodeAttr, WriteOutcome};
use crate::dispatch::context::RenameContext;
use crate::prelude::*;

/// Content generation capability — the primary interface for reading node data.
///
/// Every file node should have a `Readable` attached (though it is not
/// strictly required — a write-only file omits it). The dispatch pipeline
/// calls `read()` to produce the byte content served to FUSE `read` requests,
/// after L2 cache checks and before read middleware processing.
pub trait Readable: Send + Sync {
    /// Produce the full content of this node as a byte vector.
    ///
    /// Called on every cache miss (or every read for `CachePolicy::Never` nodes).
    /// Implementations may be cheap (static bytes) or expensive (LSP queries,
    /// template rendering), so callers should respect the node's cache policy.
    fn read(&self, ctx: &RequestContext<'_>) -> Result<Vec<u8>>;
}

/// Blanket impl so `Arc<dyn Readable>` can be used interchangeably with
/// `&dyn Readable`, enabling shared ownership of readables across nodes.
impl<T: Readable + ?Sized> Readable for Arc<T> {
    fn read(&self, ctx: &RequestContext<'_>) -> Result<Vec<u8>> { (**self).read(ctx) }
}

/// Write handling capability for nodes that accept data mutations.
///
/// Attached via [`VirtualNode::with_writable`](super::VirtualNode::with_writable).
/// The FUSE layer calls `write()` (or `truncate_write()` for `O_TRUNC` opens)
/// after write middleware processing.
pub trait Writable: Send + Sync {
    /// Handle a write of `data` to this node.
    ///
    /// Return [`WriteOutcome::Written`] on success, [`WriteOutcome::Ignored`]
    /// to silently discard the write, or [`WriteOutcome::Redirect`] to
    /// forward the write to a different VFS path.
    fn write(&self, ctx: &RequestContext<'_>, data: &[u8]) -> Result<WriteOutcome>;

    /// Handle a truncating write (`O_TRUNC` semantics).
    ///
    /// Defaults to delegating to [`write()`](Self::write). Override when
    /// truncation has different semantics than a normal write (e.g., clearing
    /// staged state before accepting new data).
    fn truncate_write(&self, ctx: &RequestContext<'_>, data: &[u8]) -> Result<WriteOutcome> { self.write(ctx, data) }
}

/// Rename capability for nodes that support `mv` operations.
///
/// The dispatch layer translates a FUSE `rename` into a call to this trait
/// on the source node. The [`RenameContext`] carries both the source and
/// target paths.
pub trait Renameable: Send + Sync {
    /// Perform the rename operation described by the context.
    fn rename(&self, ctx: &RenameContext<'_>) -> Result<()>;
}

/// Unlink (delete) capability for nodes that support `rm` operations.
///
/// The dispatch layer calls `unlink()` when a FUSE `unlink` is received
/// for a virtual node. The implementation decides what "deletion" means —
/// it may remove data, splice out a line range, or trigger other side effects.
pub trait Unlinkable: Send + Sync {
    /// Delete this node or perform the equivalent cleanup.
    fn unlink(&self, ctx: &RequestContext<'_>) -> Result<()>;
}

/// Optional lifecycle hooks for nodes that need to track open/close state.
///
/// All methods have default no-op implementations, so implementors only
/// override the hooks they care about. Useful for resources that need
/// setup/teardown (e.g., LSP sessions) or custom `getattr` responses
/// (e.g., dynamically-sized files).
pub trait Lifecycle: Send + Sync {
    /// Called when a FUSE file handle is opened for this node.
    fn open(&self, _ctx: &RequestContext<'_>) -> Result<()> { Ok(()) }

    /// Called when a FUSE file handle is released (all references closed).
    fn release(&self, _ctx: &RequestContext<'_>) -> Result<()> { Ok(()) }

    /// Return custom attribute overrides for `getattr`.
    ///
    /// Returns `None` by default, meaning the dispatch layer uses the
    /// node's standard attributes. Override to provide dynamic size,
    /// mtime, or ctime values.
    fn getattr(&self, _ctx: &RequestContext<'_>) -> Option<NodeAttr> { None }
}

/// Extended attribute (xattr) capability for nodes that expose key-value metadata.
///
/// The FUSE xattr handlers delegate to this trait after checking FUSE-level
/// attributes (like `user.error`). Providers use xattrs for out-of-band
/// metadata that does not fit in the file's content (e.g., symbol properties,
/// diagnostic counts).
pub trait Xattrable: Send + Sync {
    /// List all extended attribute names for this node.
    fn list_xattrs(&self, ctx: &RequestContext<'_>) -> Vec<String>;

    /// Get the value of an extended attribute by name.
    ///
    /// Returns `Ok(None)` if the attribute does not exist — the FUSE layer
    /// translates this to `ENODATA`.
    fn get_xattr(&self, ctx: &RequestContext<'_>, name: &str) -> Result<Option<Vec<u8>>>;

    /// Set the value of an extended attribute.
    fn set_xattr(&self, ctx: &RequestContext<'_>, name: &str, value: &[u8]) -> Result<()>;
}
