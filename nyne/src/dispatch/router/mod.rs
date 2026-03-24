//! Core FUSE operation router with caching and inode map.
//!
//! Central dispatch layer routing FUSE operations to providers with
//! L1 directory cache, L2 content cache, and inode map.
//!
//! Methods are split across submodules for clarity:
//! - `lookup` — resolution, name lookup, inode resolution
//! - `readdir` — directory listing, parent inode navigation
//! - `content` — content read/write through L2 cache + pipeline
//! - `mutation` (sibling) — create, remove, rename dispatch + real-FS fallback
//! - `invalidation` (sibling) — cache invalidation + event processing + watcher integration
//! - `resolver` (sibling) — `Resolver` trait definition + implementation + recursion guard

/// Content I/O operations (read/write through L2 cache).
mod content;
/// Resolution, name lookup, and inode resolution operations.
mod lookup;
/// Directory listing and parent inode navigation.
mod readdir;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, OnceLock};

use super::cache::{CachedNode, DirState, L1Cache, NodeEntry};
use super::content_cache::{ContentCache, FileGenerations};
use super::inode::{InodeEntry, InodeMap};
use super::invalidation::{EventSink, KernelNotifier};
use super::path_filter::PathFilter;
use super::pipeline::Pipeline;
use super::registry::ProviderRegistry;
use crate::dispatch::context::RequestContext;
use crate::node::VirtualNode;
use crate::provider::{Provider, ProviderId};
use crate::types::file_kind::FileKind;
use crate::types::real_fs::RealFs;
use crate::types::vfs_path::VfsPath;

/// Central dispatch layer that routes FUSE operations to providers.
///
/// Implements [`Resolver`](crate::dispatch::resolver::Resolver) so that
/// virtual nodes can perform cross-node lookups without depending on the
/// concrete router type.
///
/// Integrates L1 directory cache, L2 content cache, and inode map for
/// efficient FUSE lookups.
///
/// Methods are split across multiple files for clarity:
/// - `router/mod.rs` — struct definition, core helpers, cache insertion
/// - `router/lookup.rs` — resolution, name lookup, inode resolution
/// - `router/readdir.rs` — directory listing, parent inode navigation
/// - `router/content.rs` — content read/write through L2 cache + pipeline
/// - `mutation.rs` — create, remove, rename dispatch + real-FS fallback
/// - `invalidation.rs` — cache invalidation + event processing + watcher integration
/// - `resolver_impl.rs` — `Resolver` trait implementation + recursion guard
pub struct Router {
    pub(super) registry: Arc<ProviderRegistry>,
    pub(super) real_fs: Arc<dyn RealFs>,
    pub(super) events: Arc<dyn EventSink>,
    pub(super) path_filter: PathFilter,
    pub(super) pipeline: Pipeline,
    pub(super) cache: L1Cache,
    pub(super) content_cache: ContentCache,
    pub(super) file_generations: Arc<FileGenerations>,
    pub(super) inodes: InodeMap,
    /// Kernel cache notifier, set after FUSE session is created.
    ///
    /// `OnceLock` because `fuser::Notifier` is only available after mount.
    /// Before it's set, kernel invalidations are silently skipped.
    pub(super) kernel_notifier: OnceLock<Box<dyn KernelNotifier>>,
    /// Guard against re-entrant `on_fs_change` calls.
    ///
    /// Set while `handle_fs_changes` is notifying providers. If a
    /// provider's `on_fs_change` mutates the real FS, the resulting
    /// inotify events flow back through the watcher. Cache invalidation
    /// still runs, but provider notification is suppressed to prevent
    /// infinite feedback loops.
    pub(super) in_fs_change_notify: AtomicBool,
}

/// Core router construction, node insertion, and service accessors.
impl Router {
    /// The root inode number (matches `FUSE_ROOT_INODE`).
    pub const ROOT_INODE: u64 = InodeMap::ROOT_INODE;

    /// Create a new router backed by the given registry and services.
    pub fn new(
        registry: Arc<ProviderRegistry>,
        real_fs: Arc<dyn RealFs>,
        events: Arc<dyn EventSink>,
        path_filter: PathFilter,
    ) -> Self {
        let file_generations = Arc::new(FileGenerations::new());
        Self {
            registry,
            real_fs,
            events,
            path_filter,
            pipeline: Pipeline::new(),
            cache: L1Cache::new(),
            content_cache: ContentCache::new(Arc::clone(&file_generations)),
            file_generations,
            inodes: InodeMap::new(),
            kernel_notifier: OnceLock::new(),
            in_fs_change_notify: AtomicBool::new(false),
        }
    }

    /// Install a kernel notifier for FUSE cache invalidation.
    ///
    /// Must be called exactly once after the FUSE session is mounted.
    /// Panics if called twice.
    pub fn set_kernel_notifier(&self, notifier: Box<dyn KernelNotifier>) {
        assert!(
            self.kernel_notifier.set(notifier).is_ok(),
            "kernel notifier already set"
        );
    }

    /// Access the real filesystem abstraction.
    pub(crate) fn real_fs(&self) -> &dyn RealFs { self.real_fs.as_ref() }

    /// Find an active provider by its ID.
    pub(crate) fn find_provider(&self, id: ProviderId) -> Option<&Arc<dyn Provider>> { self.registry.find_provider(id) }

    /// Build a [`RequestContext`] using this router's services.
    pub fn make_request_context<'a>(&'a self, path: &'a VfsPath) -> RequestContext<'a> {
        RequestContext {
            path,
            real_fs: self.real_fs.as_ref(),
            events: self.events.as_ref(),
            resolver: self,
            file_generations: &self.file_generations,
        }
    }

    /// Insert or update a node in the cache, returning its inode number.
    ///
    /// SSOT for inode allocation + cache insertion. All paths that create
    /// cached nodes (resolve, lookup, real entry merge) go through this.
    ///
    /// **Inode stability:** If a node with the same name already exists in
    /// the directory, its inode number is reused and only the `kind`,
    /// `source`, and `generation` are replaced. This keeps inode numbers
    /// stable across cache invalidation + re-resolve cycles, preventing
    /// stale kernel references.
    ///
    /// **Generation stamping:** Children- and derived-sourced entries are
    /// stamped with the directory's current `resolve_generation` so that
    /// [`DirState::sweep_stale_resolve`] can identify entries not
    /// refreshed in the current cycle. Lookup-sourced entries use
    /// generation 0 (exempt from sweep).
    pub(super) fn insert_node(&self, dir: &mut DirState, dir_path: &VfsPath, entry: NodeEntry) -> u64 {
        let generation = if entry.source.is_generation_tracked() {
            dir.resolve_generation()
        } else {
            0
        };

        // Reuse existing inode if the name is already cached.
        if let Some(existing_inode) = dir.get(&entry.name).map(|cn| cn.inode) {
            dir.insert(entry.name, CachedNode {
                inode: existing_inode,
                kind: entry.kind,
                source: entry.source,
                generation,
            });
            return existing_inode;
        }

        let parent_inode = self.parent_inode_for_dir(dir_path);
        let inode = self.inodes.allocate(InodeEntry {
            dir_path: dir_path.clone(),
            name: entry.name.clone(),
            parent_inode,
        });
        dir.insert(entry.name, CachedNode {
            inode,
            kind: entry.kind,
            source: entry.source,
            generation,
        });
        inode
    }

    /// Determine the parent inode number for entries in a given directory.
    ///
    /// If the directory itself has an inode (i.e., it's been cached as a node
    /// in its parent), returns that inode. For root, returns `ROOT_INODE`.
    pub(super) fn parent_inode_for_dir(&self, dir_path: &VfsPath) -> u64 {
        if dir_path.is_root() {
            return Self::ROOT_INODE;
        }
        // Find this directory's own inode by looking it up in its parent.
        let parent = dir_path.parent().unwrap_or(VfsPath::root());
        let name = dir_path.name().unwrap_or("");
        self.cache
            .get(&parent)
            .and_then(|handle| {
                let dir = handle.read();
                dir.get(name).map(|cn| cn.inode)
            })
            .unwrap_or(Self::ROOT_INODE)
    }
}

/// A single entry from [`Router::collect_readdir_entries`].
pub struct ReaddirEntry {
    pub(crate) inode: u64,
    pub(crate) kind: FileKind,
    pub(crate) name: String,
}

/// Factory methods for constructing readdir entries.
impl ReaddirEntry {
    /// Create the `.` (self) directory entry.
    fn dot(inode: u64) -> Self {
        Self {
            inode,
            kind: FileKind::Directory,
            name: ".".to_owned(),
        }
    }

    /// Create the `..` (parent) directory entry.
    fn dotdot(inode: u64) -> Self {
        Self {
            inode,
            kind: FileKind::Directory,
            name: "..".to_owned(),
        }
    }

    /// Create a real filesystem entry.
    const fn real(inode: u64, kind: FileKind, name: String) -> Self { Self { inode, kind, name } }
}

/// Owned snapshot of a resolved inode — extracted from cache with no held locks.
pub enum ResolvedInode {
    Real {
        file_type: FileKind,
        path: VfsPath,
    },
    Virtual {
        node: Arc<VirtualNode>,
        provider_id: ProviderId,
        dir_path: VfsPath,
    },
}

/// Accessors for resolved inode snapshots.
impl ResolvedInode {
    /// The VFS path as a string reference, for logging.
    pub fn path_str(&self) -> &str {
        match self {
            Self::Real { path, .. } => path.as_str(),
            Self::Virtual { dir_path, node: _, .. } => {
                // dir_path is the parent; node.name() is the entry.
                // For logging, just return dir_path — avoids allocation.
                dir_path.as_str()
            }
        }
    }
}
