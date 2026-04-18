use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use color_eyre::eyre::Result;
use nyne::router::{CachePolicy, NamedNode, ReadContext, Readable, Request, StateSnapshot};
use nyne_companion::CompanionRequest;

/// Content-caching wrapper for [`Readable`].
///
/// Caches content according to the node's [`CachePolicy`]:
/// - **`Default`**: caches on first read via `OnceLock`, never expires
///   (generation-based invalidation evicts the whole entry).
/// - **`Ttl(d)`** with `d > 0`: caches with a timestamp, re-reads from
///   inner when expired.
///
/// Nodes with `CachePolicy::NoCache` (or `Ttl(Duration::ZERO)`) opt out
/// of caching entirely and are never wrapped — see [`wrap_readable`].
///
/// Shared via `Arc` across cache clones — all lookups of the same node see
/// the same cached content.
pub(super) struct CachedReadable {
    pub(in crate::provider) inner: Arc<dyn Readable>,
    pub(in crate::provider) persistent: OnceLock<Arc<[u8]>>,
    pub(in crate::provider) timed: Mutex<Option<(Instant, Arc<[u8]>)>>,
    /// `None` → persistent (`OnceLock`); `Some(d)` → timed cache with TTL `d`.
    pub(in crate::provider) ttl: Option<Duration>,
}
impl Readable for CachedReadable {
    fn read(&self, ctx: &ReadContext<'_>) -> Result<Vec<u8>> {
        let Some(ttl) = self.ttl else {
            // Persistent mode — cache forever via OnceLock.
            if let Some(content) = self.persistent.get() {
                return Ok(content.to_vec());
            }
            let content = self.inner.read(ctx)?;
            let arc: Arc<[u8]> = Arc::from(content.as_slice());
            // Race is benign: both values are correct, second init is ignored.
            let _ = self.persistent.set(Arc::clone(&arc));
            return Ok(content);
        };

        // TTL mode — check expiry, re-read when stale.
        if let Some((cached_at, ref content)) = *self.timed.lock().unwrap_or_else(PoisonError::into_inner)
            && cached_at.elapsed() < ttl
        {
            return Ok(content.to_vec());
        }
        let content = self.inner.read(ctx)?;
        *self.timed.lock().unwrap_or_else(PoisonError::into_inner) =
            Some((Instant::now(), Arc::from(content.as_slice())));
        Ok(content)
    }

    fn size(&self) -> Option<u64> {
        if self.ttl.is_none() {
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
/// - [`CachePolicy::Default`] → persistent cache (generation-based invalidation only).
/// - [`CachePolicy::NoCache`] → opt out of caching; node is **not** wrapped.
/// - [`CachePolicy::Ttl`] with `d > 0` → content cached for `d`, then re-read.
/// - [`CachePolicy::Ttl`] with `Duration::ZERO` → same as `NoCache` (not wrapped).
///
/// Skipped when the node has a `backing_path` (FUSE reads from the real file).
pub(super) fn wrap_readable(node: &mut NamedNode) {
    if node.readable().is_some_and(|r| r.backing_path().is_some()) {
        return;
    }
    let ttl: Option<Duration> = match node.cache_policy() {
        CachePolicy::NoCache => return,
        CachePolicy::Ttl(d) if d.is_zero() => return,
        CachePolicy::Ttl(d) => Some(d),
        CachePolicy::Default => None,
    };
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
