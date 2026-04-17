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
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread::Builder;

use fuser::{INodeNo, Notifier};
use tracing::{trace, warn};

use crate::fuse::inode_map::InodeMap;
use crate::path_utils::PathExt;
use crate::router::Chain;

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

/// Notifies the kernel to invalidate its page/dentry caches.
///
/// This trait abstracts `fuser::Notifier` so the dispatch layer remains
/// independent of the FUSE crate. The FUSE layer implements it with a
/// real `Notifier`; tests can use a no-op or recording implementation.
///
/// All methods are best-effort — errors are logged but never propagated.
pub trait KernelNotifier: Send + Sync {
    /// Invalidate the kernel's cached attributes and data for an inode.
    fn inval_inode(&self, inode: u64);

    /// Invalidate a directory entry (name) within a parent inode.
    fn inval_entry(&self, parent_inode: u64, name: &str);
}

/// Invalidate the kernel page/dentry cache for a file by path.
///
/// Looks up the inode for the given path and invalidates both the inode's
/// cached page data and its parent directory entry. Best-effort — silently
/// skips paths that have no allocated inode.
pub fn invalidate_inode_at(path: &Path, notifier: &dyn KernelNotifier, inodes: &InodeMap) {
    let Some((dir, name)) = path.split_dir_name() else {
        return;
    };
    if let Some(ino) = inodes.find_inode(dir, name) {
        notifier.inval_inode(ino);
    }
    if let Some((grandparent, dir_name)) = dir.split_dir_name()
        && let Some(parent_ino) = inodes.find_inode(grandparent, dir_name)
    {
        notifier.inval_entry(parent_ino, name);
    }
}

/// Propagate a batch of source path changes to providers and kernel caches.
///
/// This is the single source of truth for "a set of source paths changed —
/// invalidate everything that depends on them." Both the FUSE mutation path
/// (inline writes, via [`FuseFilesystem::notify_change`]) and the filesystem
/// watcher (external changes, via [`EventLoop::flush`]) must route through
/// this function to keep invalidation semantics identical.
///
/// The sequence is:
///
/// 1. Each provider's [`Provider::on_change`] is called with the full batch.
///    Providers bump internal caches (e.g. [`CacheProvider`]) and may return
///    derived [`InvalidationEvent`]s for dependent VFS paths.
/// 2. Every raw source path in `affected` is invalidated in the kernel via
///    [`invalidate_inode_at`] — this drops the kernel's page cache and
///    dentry entry for the file, so the next access re-enters FUSE and
///    reads fresh data. Skipping this loop (as the watcher did prior to
///    this function's introduction) leaves the kernel serving stale
///    content for externally-modified files.
/// 3. Every derived invalidation event from step 1 is also fed to
///    [`invalidate_inode_at`] so companion namespaces and other
///    provider-owned virtual paths invalidate in lockstep.
///
/// Best-effort: nothing here returns errors. Paths with no allocated inode
/// are silently skipped (they'll be freshly resolved on next access
/// anyway).
pub fn propagate_source_changes(affected: &[PathBuf], chain: &Chain, notifier: &dyn KernelNotifier, inodes: &InodeMap) {
    if affected.is_empty() {
        return;
    }
    let events: Vec<_> = chain.providers().iter().flat_map(|p| p.on_change(affected)).collect();
    for source_path in affected {
        invalidate_inode_at(source_path, notifier, inodes);
    }
    for event in &events {
        invalidate_inode_at(&event.path, notifier, inodes);
    }
}

#[cfg(test)]
mod tests;
