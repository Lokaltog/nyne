//! Kernel cache invalidation via FUSE notifications.

use std::ffi::OsStr;
use std::sync::mpsc;
use std::thread::Builder;

use fuser::{INodeNo, Notifier};
use tracing::warn;

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

/// Notification message sent to the background drain thread.
enum NotifyMsg {
    InvalInode { inode: u64 },
    InvalEntry { parent_inode: u64, name: String },
}

/// Non-blocking [`KernelNotifier`] wrapper.
///
/// Enqueues notifications into an unbounded channel drained by a dedicated
/// background thread. This prevents `writev(/dev/fuse)` from blocking the
/// caller (typically the filesystem watcher) when the kernel stalls on a
/// notification — e.g., because it needs to issue a FUSE callback but all
/// handler threads are busy.
///
/// Dropped notifications (channel disconnected) are harmless: stale kernel
/// cache entries expire via TTL and get re-resolved on next access.
pub struct AsyncNotifier {
    tx: mpsc::Sender<NotifyMsg>,
}

/// Construction for [`AsyncNotifier`].
impl AsyncNotifier {
    /// Wrap a synchronous notifier in an async channel + drain thread.
    #[allow(clippy::expect_used)] // thread spawn failure = system resource exhaustion
    pub fn new(inner: impl KernelNotifier + 'static) -> Self {
        let (tx, rx) = mpsc::channel::<NotifyMsg>();

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
    /// Enqueues an inode invalidation message.
    fn inval_inode(&self, inode: u64) { let _ = self.tx.send(NotifyMsg::InvalInode { inode }); }

    fn inval_entry(&self, parent_inode: u64, name: &str) {
        let _ = self.tx.send(NotifyMsg::InvalEntry {
            parent_inode,
            name: name.to_owned(),
        });
    }
}
