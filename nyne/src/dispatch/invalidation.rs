//! Cache invalidation and event processing for L1/L2 caches with kernel notification.

use std::sync::atomic::{AtomicBool, Ordering};

use super::router::Router;
use crate::provider::ProviderId;
use crate::types::vfs_path::VfsPath;

/// Sink for cache invalidation events.
///
/// Implementations choose between fire-and-forget (e.g., logging) and
/// buffered (drain after FUSE operations) strategies. The default
/// `drain()` returns an empty vec — override it to buffer events.
pub trait EventSink: Send + Sync {
    /// Emit an invalidation event.
    fn emit(&self, event: InvalidationEvent);

    /// Drain buffered events for deferred processing.
    ///
    /// Fire-and-forget sinks return an empty vec (the default).
    /// Buffered sinks return and clear their accumulated events.
    fn drain(&self) -> Vec<InvalidationEvent> { Vec::new() }
}

/// Events that trigger cache invalidation or re-materialization.
///
/// Emitted by providers and node capabilities via [`EventSink::emit`] during
/// FUSE operations, then drained and processed by the router after the
/// operation completes. Each variant specifies the scope of invalidation,
/// from surgical (single node) to broad (entire provider).
///
/// Marked `#[non_exhaustive]` to allow adding finer-grained invalidation
/// scopes (e.g., line-range or attribute-only) without a breaking change.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum InvalidationEvent {
    /// A single node's content changed — invalidates L2 content only.
    ///
    /// The directory structure (L1) is unaffected; the node still exists
    /// at the same path with the same inode. Only the cached byte content
    /// in the L2 [`ContentCache`](super::content_cache::ContentCache) is evicted.
    Node { provider_id: ProviderId, vpath: VfsPath },
    /// An entire subtree changed — invalidates both L1 structure and L2 content.
    ///
    /// Used when a directory's children may have changed (e.g., a source file
    /// was modified, so all its companion namespace nodes may be different).
    Subtree { path: VfsPath },
    /// Everything from a provider — invalidates all L1 and L2 entries it owns.
    ///
    /// Used when a provider's global state changes (e.g., LSP restart,
    /// configuration reload) and all its previously generated content is
    /// potentially stale.
    Provider { provider_id: ProviderId },
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

/// RAII guard that resets an [`AtomicBool`] to `false` on drop.
///
/// Ensures the flag is cleared even if the guarded scope panics.
struct AtomicBoolGuard<'a>(&'a AtomicBool);

impl Drop for AtomicBoolGuard<'_> {
    fn drop(&mut self) { self.0.store(false, Ordering::Release); }
}
/// Cache invalidation and event processing methods.
///
/// Separated from the core router to keep invalidation logic (L1/L2/kernel)
/// in one place. All methods operate on `&self` — thread-safe via
/// `RwLock<BTreeMap>` + `Arc<RwLock<DirState>>` internals.
impl Router {
    /// Invalidate a single directory, forcing re-resolution on next access.
    pub fn invalidate_dir(&self, path: &VfsPath) { self.cache.invalidate_dir(path); }

    /// Invalidate all directories under a path prefix (L1 + L2).
    pub fn invalidate_subtree(&self, path: &VfsPath) {
        // Collect inodes of affected entries before invalidating L1,
        // then clear their L2 content.
        self.cache.collect_inodes_under(path).into_iter().for_each(|ino| {
            self.content_cache.invalidate(ino);
        });
        self.cache.invalidate_subtree(path);
    }

    /// Invalidate all cached entries from a specific provider (L1 + L2).
    pub fn invalidate_provider(&self, provider_id: ProviderId) {
        self.content_cache.invalidate_provider(provider_id);
        self.cache.invalidate_provider(provider_id);
    }

    /// Clear both L1 and L2 caches.
    pub fn invalidate_all(&self) {
        self.content_cache.clear();
        self.cache.clear();
    }

    /// Drain and process all buffered invalidation events.
    ///
    /// Call this after FUSE operations that may have emitted events
    /// (writes, renames, unlinks). Fire-and-forget sinks return an
    /// empty drain, making this a no-op.
    ///
    /// For each event, internal caches are invalidated first, then
    /// the kernel is notified (if a [`KernelNotifier`] is installed).
    pub fn process_events(&self) {
        let notifier = self.kernel_notifier.get().map(AsRef::as_ref);
        for event in self.events.drain() {
            self.process_invalidation_event(&event, notifier);
        }
    }

    /// Handle real filesystem changes detected by the watcher.
    ///
    /// For each changed path:
    /// 1. **Skippable paths** (gitignored / git-internal): evicts the
    ///    specific L1 entry and kernel dentry, but skips expensive
    ///    subtree invalidation (no virtual content to invalidate).
    /// 2. **Non-skippable paths**: full subtree invalidation (L1 + L2 +
    ///    kernel) plus parent directory re-resolution.
    /// 3. Notifies all active providers via [`Provider::on_fs_change`](crate::provider::Provider::on_fs_change).
    ///
    /// Provider notification (step 3) is suppressed when already inside
    /// a provider's `on_fs_change` callback to prevent infinite feedback
    /// loops from back-propagated real-FS mutations.
    ///
    /// Called from the watcher's background thread — all operations here
    /// are thread-safe.
    pub fn handle_fs_changes(&self, changed: &[VfsPath]) {
        let notifier = self.kernel_notifier.get().map(AsRef::as_ref);

        for path in changed {
            // Bump the file's generation so L1/L2 caches lazily
            // detect staleness on next access.
            self.file_generations.bump(path);

            if self.path_filter.is_skippable(path) {
                self.handle_skippable_change(path, notifier);
            } else {
                self.handle_non_skippable_change(path, notifier);
            }
        }

        // Skip provider notification if we're already inside a provider's
        // on_fs_change — prevents infinite loops from back-propagated
        // real-FS mutations.
        if self.in_fs_change_notify.swap(true, Ordering::Acquire) {
            return;
        }
        // RAII guard: reset flag even if a provider panics.
        let _guard = AtomicBoolGuard(&self.in_fs_change_notify);

        // Notify providers after all cache invalidation is complete.
        // Providers may return additional invalidation events for
        // derived virtual content that maps to different VFS paths.
        for provider in self.registry.active_providers() {
            for event in provider.on_fs_change(changed) {
                self.process_invalidation_event(&event, notifier);
            }
        }
    }

    /// Evict L1 entry and kernel dentry for a skippable (gitignored /
    /// git-internal) path. No subtree invalidation needed since these
    /// paths have no virtual content.
    fn handle_skippable_change(&self, path: &VfsPath, notifier: Option<&dyn KernelNotifier>) {
        let Some(parent) = path.parent() else { return };
        let Some(name) = path.name() else { return };
        self.cache.remove_entry(&parent, name);
        if let Some(n) = notifier {
            n.inval_entry(self.parent_inode_for_dir(&parent), name);
        }
    }

    /// Full invalidation for a non-skippable (tracked) path change.
    ///
    /// Performs subtree invalidation (L1 + L2 + kernel), then invalidates
    /// the parent directory so it re-resolves to pick up the new/deleted entry.
    fn handle_non_skippable_change(&self, path: &VfsPath, notifier: Option<&dyn KernelNotifier>) {
        self.invalidate_subtree_with_notify(path, notifier);

        let Some(parent) = path.parent() else { return };
        self.invalidate_dir(&parent);
        if let Some(n) = notifier
            && let Some(name) = path.name()
        {
            n.inval_entry(self.parent_inode_for_dir(&parent), name);
        }
    }

    /// Apply a single invalidation event: clear caches and notify kernel.
    pub(super) fn process_invalidation_event(&self, event: &InvalidationEvent, notifier: Option<&dyn KernelNotifier>) {
        match event {
            InvalidationEvent::Node { vpath, .. } => self.invalidate_node(vpath, notifier),
            InvalidationEvent::Subtree { path } => self.invalidate_subtree_with_notify(path, notifier),
            InvalidationEvent::Provider { provider_id } => {
                self.invalidate_provider(*provider_id);
                // Provider invalidation affects potentially many inodes
                // across many directories. A targeted notify would require
                // scanning all directories — instead, individual dirs will
                // re-resolve on next access, and the short kernel TTL (1s)
                // ensures stale entries expire quickly.
            }
        }
    }

    /// Invalidate a single node's L2 content and optionally notify the kernel.
    fn invalidate_node(&self, vpath: &VfsPath, notifier: Option<&dyn KernelNotifier>) {
        let Some(parent) = vpath.parent() else { return };
        let Some(name) = vpath.name() else { return };
        let Some(handle) = self.cache.get(&parent) else { return };
        let inode = {
            let dir = handle.read();
            let Some(cn) = dir.get(name) else { return };
            cn.inode
        };
        self.content_cache.invalidate(inode);
        if let Some(n) = notifier {
            n.inval_inode(inode);
        }
    }

    /// Invalidate L1/L2 caches for a subtree and notify the kernel to flush dentries.
    fn invalidate_subtree_with_notify(&self, path: &VfsPath, notifier: Option<&dyn KernelNotifier>) {
        let affected = self.collect_subtree_entries(path);
        // Collect directory inodes *before* clearing cache — these are the
        // directories whose readdir results the kernel has cached.
        let dir_inodes = self.collect_subtree_dir_inodes(path);
        self.invalidate_subtree(path);
        let Some(n) = notifier else { return };
        for (parent_inode, name) in &affected {
            n.inval_entry(*parent_inode, name);
        }
        // Invalidate the directory inodes themselves so the kernel flushes
        // its cached readdir results and re-issues readdir on next access.
        for inode in &dir_inodes {
            n.inval_inode(*inode);
        }
    }

    /// Collect `(parent_inode, child_name)` pairs for all cached entries under a path.
    ///
    /// Used to emit targeted `inval_entry` calls before the cache is cleared.
    fn collect_subtree_entries(&self, path: &VfsPath) -> Vec<(u64, String)> {
        self.cache.collect_entries_under(path, |dir_path, name, _cn| {
            let parent_inode = self.parent_inode_for_dir(dir_path);
            (parent_inode, name.to_owned())
        })
    }

    /// Collect inodes of directories within the subtree.
    ///
    /// Used to emit `inval_inode` calls so the kernel flushes its cached
    /// readdir results for these directories.
    fn collect_subtree_dir_inodes(&self, path: &VfsPath) -> Vec<u64> {
        self.cache
            .collect_dir_inodes_under(path, |dir_path| self.parent_inode_for_dir(dir_path))
    }
}
