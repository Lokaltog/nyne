//! Bounded PID → process-name cache used to avoid repeated
//! `/proc/{pid}/comm` reads.
//!
//! A single PID can generate thousands of FUSE ops (e.g. `git status`
//! issuing batched lookups), and `read_comm` is a disk read on procfs —
//! so the first read per PID is memoised until the entry is evicted.
//! The same cache is shared with the visibility plugin so its name-rule
//! lookups reuse the cached comm instead of reading procfs a second time.
//!
//! PIDs are recycled by the OS, so we cannot keep entries forever.
//! Eviction is LRU-by-access: [`LruCache`] promotes each entry to
//! most-recently-used on every read, so frequently-referenced live
//! processes stay cached while exited PIDs age out the back.

use std::num::NonZeroUsize;

use lru::LruCache;
use parking_lot::Mutex;

use crate::process::procfs::read_comm;

/// Cached PID → comm name lookup with bounded capacity.
pub struct ProcessNameCache {
    cache: Mutex<LruCache<u32, Option<String>>>,
}

impl ProcessNameCache {
    /// Soft cap on cached entries. PIDs wrap around ~4M on modern
    /// Linux; 4K entries comfortably cover active process sets while
    /// bounding memory.
    const CAPACITY: NonZeroUsize = NonZeroUsize::new(4096).expect("4096 != 0");

    pub fn new() -> Self {
        Self {
            cache: Mutex::new(LruCache::new(Self::CAPACITY)),
        }
    }

    /// Return the cached comm for `pid`, or read it from procfs and
    /// cache the result.
    ///
    /// `None` means the PID has no entry in procfs (exited, permission
    /// denied, or procfs unavailable).
    pub fn get_or_read(&self, pid: u32) -> Option<String> {
        let mut cache = self.cache.lock();
        if let Some(name) = cache.get(&pid) {
            return name.clone();
        }
        let name = read_comm(pid);
        cache.put(pid, name.clone());
        name
    }
}

impl Default for ProcessNameCache {
    fn default() -> Self { Self::new() }
}
