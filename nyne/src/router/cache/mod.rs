use std::collections::HashMap;
use std::hash::Hash;
use std::path::PathBuf;
use std::sync::{Arc, PoisonError, RwLock};

use crate::router::GenerationMap;

/// Generation-tagged cache entry.
struct CacheEntry<V> {
    generation: u64,
    source: PathBuf,
    value: V,
}

type Store<K, V> = HashMap<K, CacheEntry<V>>;

/// A generation-aware concurrent cache.
///
/// Entries are tagged with the generation they were computed at. Lookups
/// compare generation on access; stale entries are transparently recomputed.
/// Uses `RwLock<HashMap>` (not `DashMap`) to avoid reentrancy deadlocks
/// in FUSE contexts.
pub struct GenCache<K, V> {
    store: RwLock<Store<K, V>>,
    generations: Arc<GenerationMap>,
}

impl<K: Hash + Eq + Clone, V: Clone> GenCache<K, V> {
    pub fn new(generations: Arc<GenerationMap>) -> Self {
        Self {
            store: RwLock::new(HashMap::new()),
            generations,
        }
    }

    /// Get a cached value if fresh, or compute and cache it.
    ///
    /// The `compute` closure returns both the value and the source file
    /// whose generation gates freshness. This deferred source discovery is
    /// necessary because the correct source may only be available after
    /// the middleware chain has run (e.g., companion state).
    ///
    /// On hit, freshness is checked against the **entry's stored source**,
    /// not a caller-provided one — matching the approach used by [`gc`].
    ///
    /// # Concurrency
    ///
    /// Generation is read outside the lock and `compute()` runs lock-free.
    /// This is intentionally racy to avoid holding locks during computation
    /// (which risks FUSE reentrancy deadlocks). Two benign races exist:
    ///
    /// - **Born-stale entry:** if generation is bumped during `compute()`,
    ///   the stored entry is immediately stale. Next access recomputes.
    /// - **Double-compute:** two concurrent misses for the same key both
    ///   compute; second insert overwrites first. Both values are correct
    ///   for the generation they observed. Wasted work, not incorrect data.
    pub fn get_or_compute(&self, key: K, compute: impl FnOnce() -> (V, PathBuf)) -> V {
        // Fast path: read lock, check freshness using the entry's stored source.
        {
            let guard = self.store.read().unwrap_or_else(PoisonError::into_inner);
            if let Some(entry) = guard.get(&key)
                && entry.generation == self.generations.get(&entry.source)
            {
                return entry.value.clone();
            }
        }

        // Miss or stale: compute lock-free, then store under write lock.
        let (value, source) = compute();
        let generation = self.generations.get(&source);
        self.store
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key, CacheEntry {
                generation,
                source,
                value: value.clone(),
            });
        value
    }

    /// Explicitly remove an entry (rare -- prefer lazy generation-based eviction).
    pub fn invalidate(&self, key: &K) { self.store.write().unwrap_or_else(PoisonError::into_inner).remove(key); }
}

#[cfg(test)]
mod tests;
