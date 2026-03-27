//! TTL-based cache for LSP query results.
//!
//! Avoids redundant LSP server round-trips for repeated reads of the same
//! symbol or file. Entries are keyed by [`CacheKey`] (file path + LSP method +
//! position) and expire after a configurable TTL. Values are type-erased
//! (`Box<dyn Any>`) so any result type can be cached without a serde
//! round-trip.
//!
//! Invalidation is path-prefix-based: when a file changes, all entries whose
//! key starts with that file's path are evicted. This is coarse-grained but
//! correct -- LSP results for a file are only valid for the exact source text
//! the server last saw.

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
/// Named constants for LSP method keys used in [`CacheKey::method`].
///
/// Single source of truth — all cache call sites reference these instead of
/// string literals.
impl CacheKey<'_> {
    pub const DECLARATION: &'static str = "declaration";
    pub const DEFINITION: &'static str = "definition";
    pub const HOVER: &'static str = "hover";
    pub const IMPLEMENTATION: &'static str = "implementation";
    pub const INCOMING_CALLS: &'static str = "incomingCalls";
    pub const INLAY_HINT: &'static str = "inlayHint";
    pub const OUTGOING_CALLS: &'static str = "outgoingCalls";
    pub const REFERENCES: &'static str = "references";
    pub const TYPE_DEFINITION: &'static str = "typeDefinition";
}

/// Display a cache key as `path:method:line:param`.
///
/// This format is also used as the `HashMap` key string. The path-prefix
/// property enables [`Cache::invalidate_file`] to match entries by
/// checking `key.starts_with(path)`.
impl fmt::Display for CacheKey<'_> {
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
///
/// The value is type-erased so the cache can store heterogeneous LSP result
/// types (`Vec<Location>`, `Option<Hover>`, etc.) in a single `HashMap`.
/// Callers downcast via `get::<T>` which returns `None` on type mismatch.
struct CacheEntry {
    /// Type-erased cached value, downcast by callers via `Any::downcast_ref`.
    data: Box<dyn Any + Send + Sync>,
    /// When this entry was inserted, used to compute expiry against the TTL.
    cached_at: Instant,
}

/// TTL-based cache for LSP query results.
///
/// Thread-safe via `RwLock` (reads don't block reads). Values are stored
/// as type-erased `Box<dyn Any>` — no serde round-trip on access.
pub struct Cache {
    entries: RwLock<HashMap<String, CacheEntry>>,
    ttl: Duration,
}

/// Methods for querying, caching, and invalidating LSP results.
impl Cache {
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
        trace!(target: "nyne::lsp", key = %key_str, "cache insert");
        let mut entries = self.entries.write();
        entries.insert(key_str, CacheEntry {
            data: Box::new(data),
            cached_at: Instant::now(),
        });
    }

    /// Invalidate all entries whose key starts with the given file path.
    pub(crate) fn invalidate_file(&self, path: &Path) {
        let mut prefix = path.to_string_lossy().into_owned();
        prefix.push(':');
        let mut entries = self.entries.write();
        let before = entries.len();
        entries.retain(|key, _| !key.starts_with(&prefix));
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
