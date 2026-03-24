use std::any::Any;
use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use tracing::{debug, trace};

/// Structured cache key — SSOT for key format.
///
/// Encodes file path + LSP method + position into a single string key.
/// Enforces consistent formatting instead of relying on caller discipline.
pub struct CacheKey<'a> {
    pub path: &'a Path,
    pub method: &'a str,
    pub line: u32,
    /// Secondary position parameter — column for positional queries,
    /// end-line for range-scoped queries (inlay hints).
    pub param: u32,
}

/// Display a cache key as `path:method:line:param`.
impl fmt::Display for CacheKey<'_> {
    /// Formats the value for display.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}:{}",
            self.path.display(),
            self.method,
            self.line,
            self.param
        )
    }
}

/// Single cache entry with TTL metadata.
struct CacheEntry {
    data: Box<dyn Any + Send + Sync>,
    cached_at: Instant,
}

/// TTL-based cache for LSP query results.
///
/// Thread-safe via `RwLock` (reads don't block reads). Values are stored
/// as type-erased `Box<dyn Any>` — no serde round-trip on access.
pub struct LspCache {
    entries: RwLock<HashMap<String, CacheEntry>>,
    ttl: Duration,
}

/// Methods for querying, caching, and invalidating LSP results.
impl LspCache {
    /// Create a new cache with the given time-to-live for entries.
    pub(crate) fn new(ttl: Duration) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            ttl,
        }
    }

    /// Get a cached value if it exists, hasn't expired, and matches type `T`.
    ///
    /// Returns a clone of the cached value. Expired entries are removed lazily
    /// (promoted to a write lock only when needed).
    pub(crate) fn get<T: Clone + Send + Sync + 'static>(&self, key: &CacheKey) -> Option<T> {
        self.get_with_age(key).map(|(value, _age)| value)
    }

    /// Get a cached value with its age if it exists and hasn't expired.
    ///
    /// Returns both the cloned value and elapsed time since caching.
    pub(crate) fn get_with_age<T: Clone + Send + Sync + 'static>(&self, key: &CacheKey) -> Option<(T, Duration)> {
        let key_str = key.to_string();

        // Fast path: read lock for cache hit.
        {
            let entries = self.entries.read();
            let entry = entries.get(&key_str)?;
            let age = entry.cached_at.elapsed();
            if age <= self.ttl {
                let value = entry.data.downcast_ref::<T>()?;
                trace!(target: "nyne::lsp", key = %key, "cache hit");
                return Some((value.clone(), age));
            }
        }

        // Slow path: entry expired, promote to write lock to remove it.
        trace!(target: "nyne::lsp", key = %key, "cache expired");
        let mut entries = self.entries.write();
        entries.remove(&key_str);
        None
    }

    /// Insert a value into the cache.
    pub(crate) fn insert<T: Send + Sync + 'static>(&self, key: &CacheKey, data: T) {
        let key_str = key.to_string();
        trace!(target: "nyne::lsp", key = %key, "cache insert");
        let mut entries = self.entries.write();
        entries.insert(key_str, CacheEntry {
            data: Box::new(data),
            cached_at: Instant::now(),
        });
    }

    /// Invalidate all entries whose key starts with the given file path.
    pub(crate) fn invalidate_file(&self, path: &Path) {
        let prefix = path.to_string_lossy();
        let mut entries = self.entries.write();
        let before = entries.len();
        entries.retain(|key, _| !key.starts_with(prefix.as_ref()));
        let evicted = before - entries.len();
        if evicted > 0 {
            debug!(
                target: "nyne::lsp",
                path = %path.display(),
                evicted,
                "cache invalidated",
            );
        }
    }

    /// Clear all cached entries.
    pub(crate) fn clear(&self) {
        let mut entries = self.entries.write();
        let count = entries.len();
        entries.clear();
        debug!(target: "nyne::lsp", count, "cache cleared");
    }

    /// Return the number of cached entries (for status reporting).
    pub(crate) fn len(&self) -> usize { self.entries.read().len() }

    /// Return `true` if the cache contains no entries.
    pub(crate) fn is_empty(&self) -> bool { self.len() == 0 }
}

/// Unit tests.
#[cfg(test)]
mod tests;
