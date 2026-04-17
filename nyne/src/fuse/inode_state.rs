//! Per-inode mutable state held by the FUSE filesystem.
//!
//! Bundles the three inode-keyed maps that would otherwise sit directly
//! on [`FuseFilesystem`](super::FuseFilesystem) as parallel locks:
//!
//! - **Write locks** — a per-inode `Mutex` that serialises the write
//!   pipeline so concurrent flushes cannot interleave splice edits.
//! - **Write errors** — the last error message from the flush pipeline,
//!   surfaced to userspace via the `user.error` xattr until superseded
//!   or explicitly cleared.
//! - **Atime overrides** — `setattr(atime)` is accepted but not persisted
//!   through the chain; the override is held here so subsequent `getattr`
//!   calls round-trip the value the kernel just set.
//!
//! All three maps are eagerly evicted in `release()` once the last file
//! handle for an inode closes — see [`InodeState::evict`]. No background
//! GC.
//!
//! All methods take short-lived locks internally; callers never see the
//! raw `RwLock`/`HashMap` types.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use parking_lot::{Mutex, RwLock};

/// Per-inode mutable state bundle.
///
/// Every field is keyed by inode number and populated lazily on first
/// use. See the module docstring for the individual map rationales.
#[derive(Default)]
pub struct InodeState {
    write_locks: RwLock<HashMap<u64, Arc<Mutex<()>>>>,
    write_errors: RwLock<HashMap<u64, String>>,
    atime_overrides: RwLock<HashMap<u64, SystemTime>>,
}

impl InodeState {
    /// Acquire (or lazily create) the write-serialisation mutex for `ino`.
    ///
    /// Common-case read-lock fast path; falls back to a write lock only
    /// on first access for an inode.
    pub(super) fn write_lock(&self, ino: u64) -> Arc<Mutex<()>> {
        if let Some(lock) = self.write_locks.read().get(&ino) {
            return Arc::clone(lock);
        }
        Arc::clone(self.write_locks.write().entry(ino).or_default())
    }

    /// Record a write failure for `ino`, replacing any previous message.
    pub(super) fn set_write_error(&self, ino: u64, msg: String) { self.write_errors.write().insert(ino, msg); }

    /// Clear the write error for `ino`, if any.
    pub(super) fn clear_write_error(&self, ino: u64) { self.write_errors.write().remove(&ino); }

    /// Copy the current write error message for `ino`, if any.
    pub(super) fn write_error(&self, ino: u64) -> Option<String> { self.write_errors.read().get(&ino).cloned() }

    /// Whether `ino` currently has a recorded write error.
    pub(super) fn has_write_error(&self, ino: u64) -> bool { self.write_errors.read().contains_key(&ino) }

    /// Record a kernel-supplied atime override for `ino`.
    pub(super) fn set_atime(&self, ino: u64, atime: SystemTime) { self.atime_overrides.write().insert(ino, atime); }

    /// Look up the atime override for `ino`, if any.
    pub(super) fn atime(&self, ino: u64) -> Option<SystemTime> { self.atime_overrides.read().get(&ino).copied() }

    /// Drop all per-inode state for `ino`.
    ///
    /// Called from `release()` when the last handle for `ino` closes,
    /// ensuring long-running daemons don't accumulate stale entries.
    pub(super) fn evict(&self, ino: u64) {
        self.write_locks.write().remove(&ino);
        self.write_errors.write().remove(&ino);
        self.atime_overrides.write().remove(&ino);
    }
}
