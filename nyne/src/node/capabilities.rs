use std::sync::Arc;

use color_eyre::eyre::Result;

use super::kind::{NodeAttr, WriteOutcome};
use crate::dispatch::context::{RenameContext, RequestContext};

/// Content generation capability.
pub trait Readable: Send + Sync {
    /// Generates content bytes for this node.
    fn read(&self, ctx: &RequestContext<'_>) -> Result<Vec<u8>>;
}

/// Blanket impl: `Arc<dyn Readable>` delegates to the inner readable.
impl<T: Readable + ?Sized> Readable for Arc<T> {
    /// Delegates to the inner readable's read method.
    fn read(&self, ctx: &RequestContext<'_>) -> Result<Vec<u8>> { (**self).read(ctx) }
}

/// Write handling capability.
pub trait Writable: Send + Sync {
    /// Handles a write of `data` to this node.
    fn write(&self, ctx: &RequestContext<'_>, data: &[u8]) -> Result<WriteOutcome>;
    /// Handles a truncating write (defaults to normal write).
    fn truncate_write(&self, ctx: &RequestContext<'_>, data: &[u8]) -> Result<WriteOutcome> { self.write(ctx, data) }
}

/// Rename capability.
pub trait Renameable: Send + Sync {
    /// Renames this node to the target specified in the context.
    fn rename(&self, ctx: &RenameContext<'_>) -> Result<()>;
}

/// Unlink (delete) capability.
pub trait Unlinkable: Send + Sync {
    /// Deletes this node.
    fn unlink(&self, ctx: &RequestContext<'_>) -> Result<()>;
}

/// Lifecycle hooks (open/close/getattr).
pub trait Lifecycle: Send + Sync {
    /// Called when a file handle is opened.
    fn open(&self, _ctx: &RequestContext<'_>) -> Result<()> { Ok(()) }
    /// Called when a file handle is released.
    fn release(&self, _ctx: &RequestContext<'_>) -> Result<()> { Ok(()) }
    /// Returns custom attribute overrides for getattr.
    fn getattr(&self, _ctx: &RequestContext<'_>) -> Option<NodeAttr> { None }
}

/// Extended attribute capability.
pub trait Xattrable: Send + Sync {
    /// List all extended attribute names.
    fn list_xattrs(&self, ctx: &RequestContext<'_>) -> Vec<String>;

    /// Get the value of an extended attribute.
    ///
    /// Returns `Ok(None)` if the attribute does not exist.
    fn get_xattr(&self, ctx: &RequestContext<'_>, name: &str) -> Result<Option<Vec<u8>>>;

    /// Set the value of an extended attribute.
    fn set_xattr(&self, ctx: &RequestContext<'_>, name: &str, value: &[u8]) -> Result<()>;
}
