//! Bounded PID → process-name cache used to avoid repeated
//! `/proc/{pid}/comm` reads on every FUSE request.
//!
//! A single PID can generate thousands of FUSE ops (e.g. `git status`
//! issuing batched lookups), and `read_comm` is a disk read on procfs —
//! so the first read per PID is cached for the remainder of the
//! daemon's lifetime.
//!
//! PIDs are recycled by the OS, so we cannot keep entries forever. The
//! cache evicts in insertion order once [`CAPACITY`](Self::CAPACITY) is
//! reached, dropping the single oldest entry rather than clearing the
//! whole table — a blanket clear would create a performance cliff
//! whenever the threshold is crossed.

use std::collections::{HashMap, VecDeque};

use parking_lot::Mutex;

use crate::process::procfs::read_comm;

/// Cached PID → comm name lookup with bounded capacity.
pub struct ProcessNameCache {
    inner: Mutex<Inner>,
}

struct Inner {
    map: HashMap<u32, Option<String>>,
    /// Insertion order; oldest at the front, used for FIFO eviction.
    order: VecDeque<u32>,
}

impl ProcessNameCache {
    /// Soft cap on cached entries. PIDs wrap around ~4M on modern
    /// Linux; 4K entries comfortably cover active process sets while
    /// bounding memory.
    const CAPACITY: usize = 4096;

    pub(super) fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                map: HashMap::with_capacity(Self::CAPACITY),
                order: VecDeque::with_capacity(Self::CAPACITY),
            }),
        }
    }

    /// Return the cached comm for `pid`, or read it from procfs and
    /// cache the result.
    ///
    /// `None` means the PID has no entry in procfs (exited, permission
    /// denied, or procfs unavailable).
    pub(super) fn get_or_read(&self, pid: u32) -> Option<String> {
        let mut inner = self.inner.lock();
        if let Some(name) = inner.map.get(&pid) {
            return name.clone();
        }
        let name = read_comm(pid);
        if inner.map.len() >= Self::CAPACITY
            && let Some(oldest) = inner.order.pop_front()
        {
            inner.map.remove(&oldest);
        }
        inner.map.insert(pid, name.clone());
        inner.order.push_back(pid);
        name
    }
}
