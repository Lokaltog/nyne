use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use color_eyre::eyre::Result;
use nyne::router::{NamedNode, ReadContext, Readable, Request, StateSnapshot};
use nyne_companion::CompanionRequest;

/// Content-caching wrapper for [`Readable`].
///
/// Caches content on first `read()` in a shared `OnceLock`. Subsequent calls
/// return the cached bytes, and `size()` reports the correct byte length.
/// Shared via `Arc` across cache clones — all lookups of the same node see
/// the same cached content. Invalidation is automatic: generation bumps
/// replace the cache entry, dropping the old `CachedReadable`.
pub(super) struct CachedReadable {
    pub(in crate::provider) inner: Arc<dyn Readable>,
    pub(in crate::provider) cached: OnceLock<Arc<[u8]>>,
}
impl Readable for CachedReadable {
    fn read(&self, ctx: &ReadContext<'_>) -> Result<Vec<u8>> {
        if let Some(content) = self.cached.get() {
            return Ok(content.to_vec());
        }
        let content = self.inner.read(ctx)?;
        let arc: Arc<[u8]> = Arc::from(content.as_slice());
        // Race is benign: both values are correct, second init is ignored.
        let _ = self.cached.set(Arc::clone(&arc));
        Ok(content)
    }

    fn size(&self) -> Option<u64> { self.cached.get().map(|c| c.len() as u64) }

    fn backing_path(&self) -> Option<&Path> { self.inner.backing_path() }
}
/// Wrap a node's `Readable` with [`CachedReadable`] for content caching.
///
/// Nodes with a `backing_path` are skipped — the FUSE layer reads their
/// size from filesystem metadata directly.
pub(super) fn wrap_readable(node: &mut NamedNode) {
    // Only wrap virtual readables (no backing path).
    if node.readable().is_some_and(|r| r.backing_path().is_some()) {
        return;
    }
    if let Some(inner) = node.take_readable() {
        node.set_readable(CachedReadable {
            inner,
            cached: OnceLock::new(),
        });
    }
}
/// Cached representation of a resolved node.
/// Stores a cloned `NamedNode` (now possible with Arc capabilities).
#[derive(Clone)]
pub(super) struct CachedNode(pub(super) NamedNode);
/// Cached result: value + request state snapshot.
///
/// Restoring state on cache hit ensures downstream side effects (e.g.
/// visibility overrides set by companion) are replayed correctly.
#[derive(Clone)]
pub(super) struct CachedResult<T> {
    pub(super) value: T,
    pub(super) state: StateSnapshot,
}
/// Derive the generation source from the request state.
///
/// Must be called **after** `next.run(req)` so companion state is available.
/// Returns the companion's `source_file` for companion paths, or the request
/// path itself for non-companion paths.
pub(super) fn source_from_request(req: &Request) -> PathBuf {
    req.source_file().unwrap_or_else(|| req.path().to_owned())
}
