//! L1 directory structure cache with per-directory resolve generations and invalidation.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use parking_lot::RwLock;

use crate::node::VirtualNode;
use crate::node::visibility::Visibility;
use crate::provider::ProviderId;
use crate::types::ProcessVisibility;
use crate::types::file_kind::FileKind;
use crate::types::vfs_path::VfsPath;

/// How a node entered the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NodeSource {
    /// From `Provider::children()` — visible in readdir.
    Children,
    /// From `Provider::lookup()` — hidden from readdir.
    Lookup,
    /// From plugin derivation — swept alongside `Children` on re-resolve.
    Derived,
    /// From a mutation operation (create, mkdir) — visible in readdir.
    Mutated,
}

impl NodeSource {
    /// Whether this source participates in the resolve generation lifecycle.
    ///
    /// Generation-tracked entries are stamped with the current resolve
    /// generation on insert and swept by
    /// [`DirState::sweep_stale_resolve`] when their generation is stale.
    pub(super) const fn is_generation_tracked(self) -> bool { matches!(self, Self::Children | Self::Derived) }
}

/// What kind of node this cache entry represents.
pub(super) enum CachedNodeKind {
    /// A provider-generated virtual node with capabilities.
    Virtual {
        node: Arc<VirtualNode>,
        provider_id: ProviderId,
    },
    /// A real filesystem entry — FUSE handles I/O directly.
    Real { file_type: FileKind },
}

impl CachedNodeKind {
    pub(super) fn file_kind(&self) -> FileKind {
        match self {
            Self::Real { file_type } => *file_type,
            Self::Virtual { node, .. } => node.kind().file_kind(),
        }
    }
}

/// A new node to be inserted into the directory cache.
///
/// Groups the per-node fields passed to [`Router::insert_node`].
pub(super) struct NodeEntry {
    pub name: String,
    pub kind: CachedNodeKind,
    pub source: NodeSource,
}

pub(super) struct CachedNode {
    pub(super) inode: u64,
    pub(super) kind: CachedNodeKind,
    pub(super) source: NodeSource,
    /// Resolve generation this node was last inserted/refreshed in.
    ///
    /// Used to detect stale resolve-sourced entries after re-resolution:
    /// entries with a generation older than the current cycle were not
    /// refreshed by any provider and should be swept.
    pub(super) generation: u64,
}

impl CachedNode {
    /// Whether this node is visible in readdir.
    ///
    /// Mutated nodes (user-created) are always visible. Real filesystem
    /// entries are always visible. Virtual lookup nodes are always hidden —
    /// they exist only for direct access. For other virtual sources,
    /// visibility is determined by the node's [`Visibility`] field.
    pub(super) fn is_visible(&self) -> bool {
        if matches!(self.source, NodeSource::Mutated) {
            return true;
        }
        match &self.kind {
            CachedNodeKind::Real { .. } => true,
            CachedNodeKind::Virtual { node, .. } =>
                !matches!(self.source, NodeSource::Lookup) && node.visibility() == Visibility::Readdir,
        }
    }

    /// Whether this node is owned by the given provider.
    pub(super) fn is_owned_by(&self, provider_id: ProviderId) -> bool {
        matches!(&self.kind, CachedNodeKind::Virtual { provider_id: pid, .. } if *pid == provider_id)
    }
}

pub(super) struct DirState {
    /// Nodes keyed by name. O(1) lookup, no duplicate names possible.
    nodes: HashMap<String, CachedNode>,
    resolved: bool,
    /// When `true`, no provider emitted virtual nodes for this directory.
    /// Real entries are served directly from `RealFs::read_dir()` without
    /// caching, and inodes are allocated lazily on `lookup`.
    passthrough: bool,
    /// Monotonic counter incremented on each resolve cycle.
    ///
    /// Entries inserted during a resolve cycle are stamped with this value.
    /// After all providers have contributed, entries from prior generations
    /// (resolve-sourced only) are swept — they represent nodes that a
    /// provider used to emit but no longer does.
    resolve_generation: u64,
    /// Source file and its [`FileGenerations`] generation at resolve time.
    ///
    /// Set for directories inside a companion namespace (e.g.,
    /// `file.rs@/symbols/`). When the source file's generation advances
    /// past this value, the directory is stale and must be re-resolved.
    source_generation: Option<(VfsPath, u64)>,
    /// When `true`, this directory's contents depend on external state
    /// and must be re-resolved on every access. Set during `lookup_name`
    /// when the parent entry has `CachePolicy::Never`.
    no_cache: bool,
}

impl Default for DirState {
    fn default() -> Self { Self::new() }
}

impl DirState {
    pub(super) fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            resolved: false,
            passthrough: false,
            resolve_generation: 0,
            source_generation: None,
            no_cache: false,
        }
    }

    /// Whether `resolve()` has been called for this directory.
    pub(super) const fn is_resolved(&self) -> bool { self.resolved }

    /// Whether this directory is in passthrough mode (no virtual content).
    ///
    /// Passthrough directories have no cached real entries — `readdir`
    /// reads from `RealFs` directly, and `lookup` allocates inodes lazily.
    pub(super) const fn is_passthrough(&self) -> bool { self.passthrough }

    /// Begin a new resolve cycle: bump generation and mark resolved.
    ///
    /// Entries inserted via [`NodeSource::Children`] are stamped with
    /// the new generation. After all inserts,
    /// [`sweep_stale_resolve`](Self::sweep_stale_resolve) removes
    /// entries from prior cycles.
    pub(super) const fn begin_resolve(&mut self) {
        self.resolve_generation += 1;
        self.resolved = true;
        self.passthrough = false;
    }

    /// Mark this directory as resolved in passthrough mode.
    ///
    /// No real entries are cached — they are served directly from `RealFs`.
    /// Used when no provider emits virtual nodes for this directory.
    pub(super) const fn mark_passthrough(&mut self) {
        self.resolved = true;
        self.passthrough = true;
    }

    /// Whether this directory must be re-resolved on every access.
    pub(super) const fn is_no_cache(&self) -> bool { self.no_cache }

    /// Mark this directory as no-cache — always re-resolved.
    pub(super) const fn mark_no_cache(&mut self) { self.no_cache = true; }

    /// The current resolve generation counter.
    pub(super) const fn resolve_generation(&self) -> u64 { self.resolve_generation }

    /// Mark this directory as needing re-resolution.
    ///
    /// Preserves existing entries (and their inodes) so that re-resolve
    /// can reuse inode numbers for same-name nodes, maintaining inode
    /// stability across invalidation cycles.
    pub(super) const fn mark_unresolved(&mut self) {
        self.resolved = false;
        self.passthrough = false;
    }

    /// Record the source file and its current generation for this directory.
    ///
    /// Called during resolution for directories inside a companion namespace.
    pub(super) fn set_source_generation(&mut self, source_file: VfsPath, generation: u64) {
        self.source_generation = Some((source_file, generation));
    }

    /// Check whether this directory's source file has advanced past its
    /// cached generation. Returns `true` if stale (needs re-resolution).
    pub(super) fn is_source_stale(&self, current_gen: impl FnOnce(&VfsPath) -> u64) -> bool {
        match &self.source_generation {
            Some((source_file, cached_gen)) => current_gen(source_file) > *cached_gen,
            None => false,
        }
    }

    /// Find a node by name.
    pub(super) fn get(&self, name: &str) -> Option<&CachedNode> { self.nodes.get(name) }

    /// Get all visible nodes for readdir (filtered by [`CachedNode::is_visible`]).
    pub(super) fn visible_entries(&self) -> impl Iterator<Item = (&str, &CachedNode)> {
        self.nodes
            .iter()
            .filter_map(|(name, cn)| cn.is_visible().then_some((name.as_str(), cn)))
    }

    /// Get all entries (both visible and lookup-only).
    pub(super) fn all_entries(&self) -> impl Iterator<Item = (&str, &CachedNode)> {
        self.nodes.iter().map(|(name, cn)| (name.as_str(), cn))
    }

    /// Get readdir entries filtered by process visibility level.
    ///
    /// - `All`: returns all entries (including `Visibility::Hidden` nodes).
    /// - `Default` / `None`: returns only nodes passing [`CachedNode::is_visible`].
    pub(super) fn readdir_entries(&self, visibility: ProcessVisibility) -> Vec<(&str, &CachedNode)> {
        match visibility {
            ProcessVisibility::All => self.all_entries().collect(),
            _ => self.visible_entries().collect(),
        }
    }

    /// Insert or replace a node by name (upsert).
    ///
    /// If a node with the same name already exists, it is replaced.
    /// This prevents duplicate entries from resolve/lookup races.
    pub(super) fn insert(&mut self, name: String, node: CachedNode) { self.nodes.insert(name, node); }

    /// Remove a node by name, returning its inode if it existed.
    pub(super) fn remove(&mut self, name: &str) -> Option<u64> { self.nodes.remove(name).map(|cn| cn.inode) }

    /// Move an entry from `old_name` to `new_name`, preserving its inode
    /// and kind. Returns the inode if successful, `None` if the old name
    /// didn't exist.
    pub(super) fn move_entry(&mut self, old_name: &str, new_name: String) -> Option<u64> {
        let node = self.nodes.remove(old_name)?;
        let inode = node.inode;
        self.nodes.insert(new_name, node);
        Some(inode)
    }

    /// Remove a node by name, returning the full `CachedNode` if it existed.
    pub(super) fn remove_entry(&mut self, name: &str) -> Option<CachedNode> { self.nodes.remove(name) }

    /// Remove all nodes owned by a specific provider.
    ///
    /// Retains real entries and virtual entries from other providers.
    /// Resets `resolved` so the next access triggers a full re-resolve.
    pub(super) fn remove_by_provider(&mut self, provider_id: ProviderId) {
        let before = self.nodes.len();
        self.nodes.retain(|_, cn| !cn.is_owned_by(provider_id));
        if self.nodes.len() != before {
            self.resolved = false;
        }
    }

    /// Remove children- and derived-sourced entries from prior generations.
    ///
    /// After a full resolve cycle, entries that were not refreshed (i.e.,
    /// their generation is older than the current cycle) represent nodes
    /// that a provider used to emit but no longer does. Derived entries
    /// are swept alongside children — they depend on the same base nodes.
    /// Lookup-sourced entries are preserved — they were explicitly
    /// discovered and remain valid until cache invalidation.
    ///
    /// Returns the inode numbers of swept entries so the caller can
    /// evict their L2 content cache entries.
    pub(super) fn sweep_stale_resolve(&mut self, current_generation: u64) -> Vec<u64> {
        let mut swept = Vec::new();
        self.nodes.retain(|_, cn| {
            let keep = !cn.source.is_generation_tracked() || cn.generation == current_generation;
            if !keep {
                swept.push(cn.inode);
            }
            keep
        });
        swept
    }
}

/// A handle to a directory's state in the L1 cache.
///
/// Returned by [`L1Cache::get`] and [`L1Cache::get_or_create`]. The
/// `Arc` keeps the `DirState` alive independently of the outer map lock,
/// so callers never hold the map-level lock while working with directory
/// contents.
pub(super) type DirHandle = Arc<RwLock<DirState>>;

/// The L1 directory structure cache.
///
/// Two-level locking eliminates the `DashMap` deadlocks that plagued the
/// previous design:
///
/// - **Outer `RwLock<BTreeMap>`**: protects the map structure. Read-locked
///   for lookups and prefix range queries. Write-locked only when adding
///   a new directory entry to the map.
/// - **Inner `RwLock<DirState>` per directory**: locked individually.
///   `ensure_resolved` write-locks one dir without blocking others.
///   Reads (getattr, lookup cache hit) take read locks.
/// - **`Arc` wrapper**: callers get a handle to a `DirState` without
///   holding the outer map lock — no leaked guard types, no cross-lock
///   deadlocks.
/// - **`BTreeMap`**: efficient prefix range via `.range()` — no
///   iteration, no secondary key index needed.
///
/// **Lock ordering**: always acquire the outer map lock first, release it,
/// then acquire inner directory locks. Never hold both simultaneously.
pub(super) struct L1Cache {
    dirs: RwLock<BTreeMap<VfsPath, DirHandle>>,
}

impl Default for L1Cache {
    fn default() -> Self { Self::new() }
}

impl L1Cache {
    pub(super) const fn new() -> Self {
        Self {
            dirs: RwLock::new(BTreeMap::new()),
        }
    }

    /// Get or create the `DirState` for a path.
    ///
    /// Returns an `Arc<RwLock<DirState>>` handle. The caller must acquire
    /// the inner lock as needed — no map-level lock is held on return.
    pub(super) fn get_or_create(&self, path: &VfsPath) -> DirHandle {
        // Fast path: read lock only.
        {
            let map = self.dirs.read();
            if let Some(handle) = map.get(path) {
                return Arc::clone(handle);
            }
        }
        // Slow path: write lock to insert.
        let mut map = self.dirs.write();
        Arc::clone(map.entry(path.clone()).or_default())
    }

    /// Get the `DirState` for a path (read-only handle).
    ///
    /// Returns `None` if the directory hasn't been cached yet.
    pub(super) fn get(&self, path: &VfsPath) -> Option<DirHandle> {
        let map = self.dirs.read();
        map.get(path).map(Arc::clone)
    }

    /// Invalidate a single directory, forcing re-resolution on next access.
    ///
    /// Preserves existing entries for inode stability — same-name nodes
    /// will keep their inode numbers after re-resolution.
    pub(super) fn invalidate_dir(&self, path: &VfsPath) {
        if let Some(handle) = self.get(path) {
            handle.write().mark_unresolved();
        }
    }

    /// Remove a specific entry from a directory's cache, if it exists.
    ///
    /// Unlike [`invalidate_dir`](Self::invalidate_dir) (which marks the whole
    /// directory for re-resolution), this surgically removes a single entry.
    /// Used for inline cache eviction after real-file mutations — the entry
    /// must be gone immediately so subsequent lookups don't find a phantom.
    ///
    /// No-op if the directory or entry doesn't exist in the cache.
    pub(super) fn remove_entry(&self, dir_path: &VfsPath, name: &str) -> Option<u64> {
        self.get(dir_path)?.write().remove(name)
    }

    /// Invalidate everything under a path prefix.
    ///
    /// Preserves existing entries for inode stability.
    pub(super) fn invalidate_subtree(&self, path: &VfsPath) {
        for handle in self.handles_under(path) {
            handle.write().mark_unresolved();
        }
    }

    /// Invalidate all entries from a specific provider across all directories.
    pub(super) fn invalidate_provider(&self, provider_id: ProviderId) {
        let handles: Vec<DirHandle> = self.dirs.read().values().map(Arc::clone).collect();
        for handle in handles {
            handle.write().remove_by_provider(provider_id);
        }
    }

    /// Collect all inode numbers for entries under a path prefix.
    ///
    /// Used by the router to invalidate L2 content cache entries
    /// when a subtree is invalidated.
    pub(super) fn collect_inodes_under(&self, path: &VfsPath) -> Vec<u64> {
        let mut inodes = Vec::new();
        for handle in self.handles_under(path) {
            let dir = handle.read();
            inodes.extend(dir.all_entries().map(|(_, cn)| cn.inode));
        }
        inodes
    }

    /// Collect mapped results for all entries under a path prefix.
    ///
    /// The callback receives `(dir_path, entry_name, cached_node)` for each
    /// entry and returns a mapped value. Used by the router for kernel notify.
    pub(super) fn collect_entries_under<T>(
        &self,
        path: &VfsPath,
        mut f: impl FnMut(&VfsPath, &str, &CachedNode) -> T,
    ) -> Vec<T> {
        let mut result = Vec::new();
        for (dir_path, handle) in self.keyed_handles_under(path) {
            let dir = handle.read();
            for (name, cn) in dir.all_entries() {
                result.push(f(&dir_path, name, cn));
            }
        }
        result
    }

    /// Collect directory inodes under a path prefix.
    ///
    /// The callback maps each matching directory path to its inode.
    /// Used to emit `inval_inode` so the kernel flushes readdir caches.
    pub(super) fn collect_dir_inodes_under(
        &self,
        path: &VfsPath,
        mut resolve_inode: impl FnMut(&VfsPath) -> u64,
    ) -> Vec<u64> {
        self.keys_under(path)
            .into_iter()
            .map(|key| resolve_inode(&key))
            .collect()
    }

    /// Clear the entire cache.
    pub(super) fn clear(&self) { self.dirs.write().clear(); }

    /// Snapshot directory paths matching a prefix.
    ///
    /// Uses `BTreeMap::range()` for efficient prefix scanning — no
    /// full iteration needed.
    fn keys_under(&self, prefix: &VfsPath) -> Vec<VfsPath> {
        let map = self.dirs.read();
        Self::range_for_prefix(&map, prefix).map(|(k, _)| k.clone()).collect()
    }

    /// Collect `Arc` handles for all directories under a prefix.
    ///
    /// The outer map lock is released before any inner locks are touched.
    fn handles_under(&self, prefix: &VfsPath) -> Vec<DirHandle> {
        let map = self.dirs.read();
        Self::range_for_prefix(&map, prefix)
            .map(|(_, v)| Arc::clone(v))
            .collect()
    }

    /// Collect `(path, handle)` pairs for all directories under a prefix.
    fn keyed_handles_under(&self, prefix: &VfsPath) -> Vec<(VfsPath, DirHandle)> {
        let map = self.dirs.read();
        Self::range_for_prefix(&map, prefix)
            .map(|(k, v)| (k.clone(), Arc::clone(v)))
            .collect()
    }

    /// Compute the `BTreeMap` range for a `VfsPath` prefix.
    ///
    /// For root prefix: returns all entries (unbounded range).
    /// For non-root: returns entries in `[prefix, prefix_successor)` where
    /// `prefix_successor` is the prefix string with its last byte incremented
    /// (since `/` = 0x2F, incrementing to `0` = 0x30 captures all paths that
    /// start with `prefix/`).
    fn range_for_prefix<'a>(
        map: &'a BTreeMap<VfsPath, DirHandle>,
        prefix: &VfsPath,
    ) -> impl Iterator<Item = (&'a VfsPath, &'a DirHandle)> {
        if prefix.is_root() {
            // Root prefix matches everything.
            return map.range::<VfsPath, _>(..);
        }
        // Range: [prefix, upper_bound)
        // prefix itself is included (exact match).
        // Children have the form "prefix/..." — the '/' separator (0x2F)
        // sorts before '0' (0x30), so appending '0' to the prefix forms
        // an exclusive upper bound that captures prefix + all children.
        let mut upper = prefix.as_str().to_owned();
        upper.push('0'); // '0' = 0x30, one past '/' = 0x2F in sort order
        if let Ok(upper_path) = VfsPath::new(&upper) {
            return map.range(prefix.clone()..upper_path);
        }
        // Fallback: scan everything (should never happen with valid paths).
        map.range::<VfsPath, _>(..)
    }
}

#[cfg(test)]
mod tests;
