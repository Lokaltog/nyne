use color_eyre::eyre::Result;

use super::capabilities::{Readable, Writable};
use super::kind::WriteOutcome;
use crate::dispatch::context::RequestContext;
use crate::types::vfs_path::VfsPath;

/// Simple readable that returns static content.
pub struct StaticContent(pub &'static [u8]);

/// Readable implementation for [`StaticContent`].
impl Readable for StaticContent {
    /// Returns the static byte slice as owned content.
    fn read(&self, _ctx: &RequestContext<'_>) -> Result<Vec<u8>> { Ok(self.0.to_vec()) }
}

/// Reads and writes a real file via [`RealFs`](crate::types::real_fs::RealFs).
///
/// Captures the file's `VfsPath` at construction time. On read/write,
/// delegates to `ctx.real_fs` — the provider that creates this node
/// controls which real file it points to.
#[derive(Clone)]
pub struct PassthroughContent {
    path: VfsPath,
}

/// Construction for [`PassthroughContent`].
impl PassthroughContent {
    /// Creates a new passthrough content handler for the given path.
    pub const fn new(path: VfsPath) -> Self { Self { path } }
}

/// Readable implementation for [`PassthroughContent`].
impl Readable for PassthroughContent {
    /// Reads the real file content via the request context's filesystem.
    fn read(&self, ctx: &RequestContext<'_>) -> Result<Vec<u8>> {
        tracing::debug!(path = %self.path, "passthrough read");
        ctx.real_fs.read(&self.path)
    }
}

/// Writable implementation for [`PassthroughContent`].
impl Writable for PassthroughContent {
    /// Writes data to the real file via the request context's filesystem.
    fn write(&self, ctx: &RequestContext<'_>, data: &[u8]) -> Result<WriteOutcome> {
        tracing::debug!(path = %self.path, bytes = data.len(), "passthrough write");
        ctx.real_fs.write(&self.path, data)?;
        Ok(WriteOutcome::Written(data.len()))
    }
}
