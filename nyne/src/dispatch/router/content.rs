//! Content I/O operations.

use std::sync::Arc;

use color_eyre::eyre::Result;

use super::Router;
use crate::dispatch::cache::CachedNodeKind;
use crate::dispatch::context::RequestContext;
use crate::dispatch::write_mode::WriteMode;
use crate::node::{CachePolicy, VirtualNode, WriteOutcome};
use crate::provider::Provider;

impl Router {
    /// Read content for a virtual inode through the L2 cache.
    ///
    /// Detects stale nodes by comparing their source generation against
    /// the current [`FileGenerations`] counter. When stale, evicts the
    /// parent directory from L1, invalidates L2 + kernel page cache,
    /// re-resolves, and reads from the fresh node.
    pub(crate) fn read_content(
        &self,
        inode: u64,
        node: &VirtualNode,
        provider: &dyn Provider,
        ctx: &RequestContext<'_>,
    ) -> Result<Vec<u8>> {
        // Check node-level staleness before reading.
        if let Some((source_file, created_gen)) = node.source()
            && self.file_generations.get(source_file) > created_gen
        {
            return self.revalidate_and_read(inode, node.name(), ctx);
        }
        self.read_content_unchecked(inode, node, provider, ctx)
    }

    /// Read content without staleness checks — used after re-resolution
    /// to avoid infinite recursion.
    fn read_content_unchecked(
        &self,
        inode: u64,
        node: &VirtualNode,
        provider: &dyn Provider,
        ctx: &RequestContext<'_>,
    ) -> Result<Vec<u8>> {
        let cacheable = node.cache_policy() == CachePolicy::Cache;
        if cacheable && let Some(cached) = self.content_cache.get(inode) {
            return Ok((*cached).clone());
        }
        let source_file = node.source().map(|(f, _)| f);
        let data = self.pipeline.execute_read(node, provider, ctx)?;
        if cacheable {
            self.content_cache
                .insert(inode, data.clone(), provider.id(), source_file);
        }
        Ok(data)
    }

    /// Evict stale caches, re-resolve the parent directory, and read
    /// fresh content from the newly-created node.
    fn revalidate_and_read(&self, inode: u64, node_name: &str, ctx: &RequestContext<'_>) -> Result<Vec<u8>> {
        // Evict L1 + L2 + kernel page cache.
        self.cache.invalidate_dir(ctx.path);
        self.content_cache.invalidate(inode);
        if let Some(notifier) = self.kernel_notifier.get() {
            notifier.inval_inode(inode);
        }

        // Re-resolve: providers re-emit fresh nodes for this directory.
        self.ensure_resolved(ctx)?;

        // Look up the fresh node from L1 by name.
        let handle = self
            .cache
            .get(ctx.path)
            .ok_or_else(|| color_eyre::eyre::eyre!("directory vanished after re-resolve: {}", ctx.path))?;
        let dir = handle.read();
        let cached = dir
            .get(node_name)
            .ok_or_else(|| color_eyre::eyre::eyre!("node {node_name} vanished after re-resolve in {}", ctx.path))?;
        let CachedNodeKind::Virtual { ref node, provider_id } = cached.kind else {
            color_eyre::eyre::bail!("node {node_name} became real after re-resolve in {}", ctx.path);
        };
        let node = Arc::clone(node);
        drop(dir);

        let provider = self
            .find_provider(provider_id)
            .ok_or_else(|| color_eyre::eyre::eyre!("provider {provider_id} vanished after re-resolve"))?;
        self.read_content_unchecked(inode, &node, provider.as_ref(), ctx)
    }

    pub(crate) fn content_size(
        &self,
        inode: u64,
        node: &VirtualNode,
        provider: &dyn Provider,
        ctx: &RequestContext<'_>,
    ) -> u64 {
        if let Some(size) = self.content_cache.get_size(inode) {
            return size;
        }
        // Cache miss — read through the pipeline to populate L2.
        // On failure, return 1 so the kernel still attempts a read
        // (st_size=0 causes tools to skip reading the file entirely).
        self.read_content(inode, node, provider, ctx)
            .map_or(1, |data| data.len() as u64)
    }

    /// Write content for a virtual inode through the write pipeline.
    ///
    /// Executes the full write pipeline, invalidates the L2 content
    /// cache, and bumps the source file's generation so that sibling
    /// companion entries are lazily detected as stale on next access.
    #[allow(clippy::too_many_arguments)] // internal dispatch: inode + node + provider + write data + context
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
