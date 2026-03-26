//! Kernel cache invalidation via FUSE notifications.
//!
//! When a provider invalidates a node (e.g., source file changed on disk),
//! the kernel's cached dentries and page data must be evicted so the next
//! access returns fresh content. This module provides two [`KernelNotifier`]
//! implementations:
//!
//! - [`FuseNotifier`] — synchronous, calls `writev(/dev/fuse)` directly.
//! - [`AsyncNotifier`] — wraps any notifier in a background drain thread,
//!   preventing the caller from blocking when the kernel stalls on a
//!   notification (e.g., all FUSE handler threads are busy).
//!
//! Both are best-effort: dropped or failed notifications are harmless since
//! stale cache entries expire via TTL and get re-resolved on next access.

use std::ffi::OsStr;
use std::sync::mpsc;
use std::thread::Builder;

use fuser::{INodeNo, Notifier};
use tracing::{trace, warn};

use crate::dispatch::invalidation::KernelNotifier;

/// [`KernelNotifier`] backed by a `fuser::Notifier`.
///
/// Obtained from `BackgroundSession::notifier()` after mount and injected
/// into the router. All calls are best-effort — kernel notify failures
/// are logged but don't propagate errors.
pub struct FuseNotifier {
    inner: Notifier,
}

/// Construction for [`FuseNotifier`].
impl FuseNotifier {
    /// Wraps a `fuser::Notifier` for kernel cache invalidation.
    pub const fn new(notifier: Notifier) -> Self { Self { inner: notifier } }
}

/// [`KernelNotifier`] implementation backed by a `fuser::Notifier`.
impl KernelNotifier for FuseNotifier {
    /// Invalidates all cached data for the given inode.
    fn inval_inode(&self, inode: u64) {
        // offset=-1, len=0 → invalidate all cached data for this inode.
        if let Err(e) = self.inner.inval_inode(INodeNo(inode), -1, 0) {
            warn!(target: "nyne::fuse", inode, error = %e, "kernel inval_inode failed");
        }
    }

    /// Enqueues a directory entry invalidation message.
    fn inval_entry(&self, parent_inode: u64, name: &str) {
        if let Err(e) = self.inner.inval_entry(INodeNo(parent_inode), OsStr::new(name)) {
            warn!(target: "nyne::fuse", parent_inode, name, error = %e, "kernel inval_entry failed");
        }
    }
}

/// Notification message sent to the [`AsyncNotifier`]'s background drain thread.
///
/// Variants mirror the [`KernelNotifier`] trait methods. The `name` field
/// in `InvalEntry` is owned because the message crosses a thread boundary.
enum NotifyMsg {
    /// Invalidate all cached data for the given inode.
    InvalInode { inode: u64 },
    /// Invalidate a directory entry (name within a parent inode).
    InvalEntry { parent_inode: u64, name: String },
}

/// Non-blocking [`KernelNotifier`] wrapper.
///
/// Enqueues notifications into a bounded channel drained by a dedicated
/// background thread. This prevents `writev(/dev/fuse)` from blocking the
/// caller (typically the filesystem watcher) when the kernel stalls on a
/// notification — e.g., because it needs to issue a FUSE callback but all
/// handler threads are busy.
///
/// Dropped notifications (channel full or disconnected) are harmless: stale
/// kernel cache entries expire via TTL and get re-resolved on next access.
pub struct AsyncNotifier {
    tx: mpsc::SyncSender<NotifyMsg>,
}

/// Construction for [`AsyncNotifier`].
impl AsyncNotifier {
    /// Wraps a synchronous notifier in an unbounded channel with a background drain thread.
    ///
    /// The drain thread runs until the [`AsyncNotifier`] is dropped (channel
    /// disconnects), at which point it exits cleanly. Thread spawn failure
    /// panics — it indicates system resource exhaustion.
    #[allow(clippy::expect_used)] // thread spawn failure = system resource exhaustion
    pub fn new(inner: impl KernelNotifier + 'static) -> Self {
        let (tx, rx) = mpsc::sync_channel::<NotifyMsg>(1024);

        Builder::new()
            .name("fuse-notify".into())
            .spawn(move || {
                for msg in rx {
                    match msg {
                        NotifyMsg::InvalInode { inode } => inner.inval_inode(inode),
                        NotifyMsg::InvalEntry { parent_inode, name } => inner.inval_entry(parent_inode, &name),
                    }
                }
            })
            .expect("failed to spawn fuse-notify thread");

        Self { tx }
    }
}

/// [`KernelNotifier`] implementation that enqueues notifications asynchronously.
impl KernelNotifier for AsyncNotifier {
    /// Enqueues an inode invalidation message (best-effort, drops if queue is full).
    fn inval_inode(&self, inode: u64) {
        if let Err(mpsc::TrySendError::Full(_)) = self.tx.try_send(NotifyMsg::InvalInode { inode }) {
            trace!(target: "nyne::fuse", inode, "notification queue full, dropping inval_inode");
        }
    }

    /// Sends an entry invalidation message to the background notification thread.
    /// Best-effort: drops the notification if the queue is full.
    fn inval_entry(&self, parent_inode: u64, name: &str) {
        if let Err(mpsc::TrySendError::Full(_)) = self.tx.try_send(NotifyMsg::InvalEntry {
            parent_inode,
            name: name.to_owned(),
        }) {
            trace!(target: "nyne::fuse", parent_inode, name, "notification queue full, dropping inval_entry");
        }
    }
}
