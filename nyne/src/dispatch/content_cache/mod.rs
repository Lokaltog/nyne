//! L2 content cache for file data and generated content.
//!
//! Includes [`FileGenerations`] — a monotonic per-file generation counter
//! used by both L1 and L2 caches to detect staleness after writes.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::provider::ProviderId;
use crate::types::vfs_path::VfsPath;

/// Monotonic per-file generation counter for cache freshness.
///
/// When a real file is modified (by a VFS write or detected by the
/// file watcher), its generation is bumped.  L1 and L2 cache entries
/// record the generation at creation time and compare on access —
/// a mismatch means the entry is stale and must be recomputed.
pub struct FileGenerations {
    generations: RwLock<HashMap<VfsPath, u64>>,
}

/// Construction, bumping, and querying of per-file generation counters.
impl FileGenerations {
    /// Create an empty generation counter map.
    pub(crate) fn new() -> Self {
        Self {
            generations: RwLock::new(HashMap::new()),
        }
    }

    /// Bump the generation for `path`, returning the new value.
    pub(crate) fn bump(&self, path: &VfsPath) -> u64 {
        let mut map = self.generations.write();
        let entry = map.entry(path.clone()).or_insert(0);
        *entry += 1;
        *entry
    }

    /// Current generation for `path` (0 if never bumped).
    pub(crate) fn get(&self, path: &VfsPath) -> u64 { self.generations.read().get(path).copied().unwrap_or(0) }
}

/// Default implementation for `FileGenerations`.
impl Default for FileGenerations {
    /// Delegates to [`FileGenerations::new`].
    fn default() -> Self { Self::new() }
}

/// A cached content entry in the L2 cache.
struct ContentEntry {
    data: Arc<[u8]>,
    provider_id: ProviderId,
    /// Source file and its generation at cache time.
    /// `None` for entries not derived from a specific source file.
    source_generation: Option<(VfsPath, u64)>,
}

/// L2 content cache — caches generated file content by inode.
///
/// Populated by the read pipeline on cache miss, invalidated by
/// staleness checks against [`FileGenerations`], or explicitly by
/// `InvalidationEvent::Node`, `Subtree`, or `Provider` events.
pub(super) struct ContentCache {
    entries: RwLock<HashMap<u64, ContentEntry>>,
    file_generations: Arc<FileGenerations>,
}

/// L2 content cache operations: get, insert, invalidate, staleness checks.
impl ContentCache {
    /// Create an empty content cache backed by the given generation tracker.
    pub(super) fn new(file_generations: Arc<FileGenerations>) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            file_generations,
        }
    }

    /// Return the cached data for `inode` if it exists and is fresh, evicting stale entries.
    fn get_if_fresh(&self, inode: u64) -> Option<Arc<[u8]>> {
        {
            let entries = self.entries.read();
            let entry = entries.get(&inode)?;
            if !self.is_stale(entry) {
                return Some(Arc::clone(&entry.data));
            }
        }
        self.entries.write().remove(&inode);
        None
    }

    /// Get cached content for an inode.
    ///
    /// Returns `None` (and evicts the entry) if the source file has
    /// been modified since the entry was cached.
    pub(super) fn get(&self, inode: u64) -> Option<Arc<[u8]>> { self.get_if_fresh(inode) }

    /// Get the cached content size for an inode without cloning the data.
    ///
    /// Returns `None` (and evicts) if stale.
    pub(super) fn get_size(&self, inode: u64) -> Option<u64> { self.get_if_fresh(inode).map(|data| data.len() as u64) }

    /// Store content for an inode, returning an `Arc` to the cached data.
    ///
    /// `source_file` is the real file this content was derived from (if
    /// any). The current generation is recorded for staleness checks.
    pub(super) fn insert(
        &self,
        inode: u64,
        data: Vec<u8>,
        provider_id: ProviderId,
        source_file: Option<&VfsPath>,
    ) -> Arc<[u8]> {
        let source_generation = source_file.map(|sf| {
            let generation = self.file_generations.get(sf);
            (sf.clone(), generation)
        });
        let arc: Arc<[u8]> = Arc::from(data);
        let result = Arc::clone(&arc);
        self.entries.write().insert(inode, ContentEntry {
            data: arc,
            provider_id,
            source_generation,
        });
        result
    }

    /// Invalidate a single inode's content.
    pub(super) fn invalidate(&self, inode: u64) { self.entries.write().remove(&inode); }

    /// Invalidate all content from a specific provider.
    pub(super) fn invalidate_provider(&self, provider_id: ProviderId) {
        self.entries.write().retain(|_, entry| entry.provider_id != provider_id);
    }

    /// Clear all cached content.
    pub(super) fn clear(&self) { self.entries.write().clear(); }

    /// Check whether an entry's source file has been modified since caching.
    fn is_stale(&self, entry: &ContentEntry) -> bool {
        match &entry.source_generation {
            Some((source_file, cached_gen)) => self.file_generations.get(source_file) > *cached_gen,
            None => false,
        }
    }
}

/// Unit tests for the L2 content cache and file generations.
#[cfg(test)]
mod tests;
