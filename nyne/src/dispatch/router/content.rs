//! Content I/O operations.

use std::sync::Arc;

use color_eyre::eyre::Result;

use super::Router;
use crate::dispatch::WriteMode;
use crate::dispatch::context::RequestContext;
use crate::node::{CachePolicy, VirtualNode, WriteOutcome};
use crate::provider::Provider;

/// Content I/O operations: read/write through the L2 cache and pipeline.
impl Router {
    /// Read content for a virtual inode through the L2 cache.
    ///
    /// Content freshness is enforced by two complementary mechanisms:
    /// - **Structural:** `ensure_resolved` (called during lookup/readdir)
    ///   detects source-file staleness via `DirState::is_source_stale` and
    ///   re-resolves the directory, producing fresh nodes.
    /// - **Content:** `ContentCache::get` checks `FileGenerations` and
    ///   evicts stale entries, causing a pipeline re-run.
    ///
    /// With derived inodes having TTL=0, every access hits the daemon and
    /// passes through `ensure_resolved` before reaching this method, so
    /// the node reference is always structurally fresh.
    pub(crate) fn read_content(
        &self,
        inode: u64,
        node: &VirtualNode,
        provider: &dyn Provider,
        ctx: &RequestContext<'_>,
    ) -> Result<Arc<Vec<u8>>> {
        let cacheable = node.cache_policy() == CachePolicy::Cache;
        if cacheable && let Some(cached) = self.content_cache.get(inode) {
            return Ok(cached);
        }
        let source_file = node.source().map(|(f, _)| f);
        let data = self.pipeline.execute_read(node, provider, ctx)?;
        if cacheable {
            return Ok(self.content_cache.insert(inode, data, provider.id(), source_file));
        }
        Ok(Arc::new(data))
    }

    /// Get the L2 cached content size for an inode, if available.
    ///
    /// This is a cheap lookup-only operation — it does not run the read
    /// pipeline on cache miss. Used by `build_attr` to report `st_size`
    /// without the cost of a full pipeline execution. With `FOPEN_DIRECT_IO`,
    /// `st_size` is advisory (the kernel reads until EOF), so a cache miss
    /// simply returns `None` and the caller uses a sentinel value.
    pub(crate) fn content_cache_size(&self, inode: u64) -> Option<u64> { self.content_cache.get_size(inode) }

    /// Write content for a virtual inode through the write pipeline.
    ///
    /// Executes the full write pipeline, invalidates the L2 content
    /// cache, and bumps the source file's generation so that sibling
    /// companion entries are lazily detected as stale on next access.
    #[allow(clippy::too_many_arguments)] // internal dispatch: inode + node + provider + write data + context
    /// Writes content to a node, invalidating caches and bumping file generations.
    pub(crate) fn write_content(
        &self,
        inode: u64,
        node: &VirtualNode,
        provider: &dyn Provider,
        data: &[u8],
        mode: WriteMode,
        ctx: &RequestContext<'_>,
    ) -> Result<WriteOutcome> {
        let outcome = self.pipeline.execute_write(node, provider, data, mode, ctx)?;
        self.content_cache.invalidate(inode);
        if let Some((source_file, _)) = node.source() {
            self.file_generations.bump(source_file);
        }
        Ok(outcome)
    }
}
