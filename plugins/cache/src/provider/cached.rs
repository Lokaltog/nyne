use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use color_eyre::eyre::Result;
use nyne::router::{NamedNode, ReadContext, Readable, Request, StateSnapshot};
use nyne_companion::CompanionRequest;

/// Content-caching wrapper for [`Readable`].
///
/// Caches content according to the node's [`CachePolicy`]:
/// - **No TTL** (`persistent`): caches on first read via `OnceLock`, never
///   expires (generation-based invalidation evicts the whole entry).
/// - **TTL > 0**: caches with a timestamp, re-reads from inner when expired.
///
/// Shared via `Arc` across cache clones — all lookups of the same node see
/// the same cached content.
pub(super) struct CachedReadable {
    pub(in crate::provider) inner: Arc<dyn Readable>,
    pub(in crate::provider) persistent: OnceLock<Arc<[u8]>>,
    pub(in crate::provider) timed: Mutex<Option<(Instant, Arc<[u8]>)>>,
    pub(in crate::provider) ttl: Duration,
}
impl Readable for CachedReadable {
    fn read(&self, ctx: &ReadContext<'_>) -> Result<Vec<u8>> {
        if self.ttl == Duration::ZERO {
            // Persistent mode — cache forever via OnceLock.
            if let Some(content) = self.persistent.get() {
                return Ok(content.to_vec());
            }
            let content = self.inner.read(ctx)?;
            let arc: Arc<[u8]> = Arc::from(content.as_slice());
            // Race is benign: both values are correct, second init is ignored.
            let _ = self.persistent.set(Arc::clone(&arc));
            return Ok(content);
        }

        // TTL mode — check expiry, re-read when stale.
        if let Some((cached_at, ref content)) = *self.timed.lock().unwrap_or_else(PoisonError::into_inner)
            && cached_at.elapsed() < self.ttl
        {
            return Ok(content.to_vec());
        }
        let content = self.inner.read(ctx)?;
        *self.timed.lock().unwrap_or_else(PoisonError::into_inner) =
            Some((Instant::now(), Arc::from(content.as_slice())));
        Ok(content)
    }

    fn size(&self) -> Option<u64> {
        if self.ttl == Duration::ZERO {
            self.persistent.get().map(|c| c.len() as u64)
        } else {
            self.timed
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
                .map(|(_, c)| c.len() as u64)
        }
    }

    fn backing_path(&self) -> Option<&Path> { self.inner.backing_path() }
}
/// Wrap a node's `Readable` with [`CachedReadable`] for content caching.
///
/// Respects the node's [`CachePolicy`]:
/// - **No policy** → persistent cache (generation-based invalidation only).
/// - **`persistent()`** (ttl = None) → same as no policy.
/// - **`with_ttl(d)`** where `d > 0` → content cached for `d`, then re-read.
///
/// Skipped when the node has a `backing_path` (FUSE reads from the real file).
pub(super) fn wrap_readable(node: &mut NamedNode) {
    if node.readable().is_some_and(|r| r.backing_path().is_some()) {
        return;
    }
    // Read policy before take_readable borrows node mutably.
    let ttl = node.cache_policy().and_then(|p| p.ttl).unwrap_or(Duration::ZERO);
    if let Some(inner) = node.take_readable() {
        node.set_readable(CachedReadable {
            inner,
            persistent: OnceLock::new(),
            timed: Mutex::new(None),
            ttl,
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
