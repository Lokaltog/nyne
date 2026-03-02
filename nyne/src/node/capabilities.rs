use std::sync::Arc;

use color_eyre::eyre::Result;

use super::kind::{NodeAttr, WriteOutcome};
use crate::dispatch::context::{RenameContext, RequestContext};

/// Content generation capability.
pub trait Readable: Send + Sync {
    fn read(&self, ctx: &RequestContext<'_>) -> Result<Vec<u8>>;
}

/// Blanket impl: `Arc<dyn Readable>` delegates to the inner readable.
impl<T: Readable + ?Sized> Readable for Arc<T> {
    fn read(&self, ctx: &RequestContext<'_>) -> Result<Vec<u8>> { (**self).read(ctx) }
}

/// Write handling capability.
pub trait Writable: Send + Sync {
    fn write(&self, ctx: &RequestContext<'_>, data: &[u8]) -> Result<WriteOutcome>;
    fn truncate_write(&self, ctx: &RequestContext<'_>, data: &[u8]) -> Result<WriteOutcome> { self.write(ctx, data) }
}

/// Rename capability.
pub trait Renameable: Send + Sync {
    fn rename(&self, ctx: &RenameContext<'_>) -> Result<()>;
}

/// Unlink (delete) capability.
pub trait Unlinkable: Send + Sync {
    fn unlink(&self, ctx: &RequestContext<'_>) -> Result<()>;
}

/// Lifecycle hooks (open/close/getattr).
pub trait Lifecycle: Send + Sync {
    fn open(&self, _ctx: &RequestContext<'_>) -> Result<()> { Ok(()) }
    fn release(&self, _ctx: &RequestContext<'_>) -> Result<()> { Ok(()) }
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
