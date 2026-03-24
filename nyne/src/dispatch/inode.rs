//! Bidirectional inode number <-> VFS location mapping with growth-only semantics.

use parking_lot::RwLock;
use slab::Slab;

use crate::types::vfs_path::VfsPath;

/// Metadata for a single inode.
///
/// Stores the location (directory + name + parent inode) for O(1)
/// inode→location lookup without scanning all cached directories.
/// The L1 cache is the source of truth for node existence and provider ownership.
#[derive(Clone)]
pub(super) struct InodeEntry {
    /// Directory containing this node.
    pub(super) dir_path: VfsPath,
    /// Name of this node within its directory.
    pub(super) name: String,
    /// Inode of the parent directory (`ROOT_INODE` for root's children).
    pub(super) parent_inode: u64,
}

/// Bidirectional inode number <-> location mapping.
///
/// FUSE reserves inode 0 (unused) and inode 1 (`FUSE_ROOT_INODE`).
/// Virtual node inodes start at 2. Internally, a `Slab` provides O(1)
/// allocation and lookup; inode numbers are `slab_index + Self::INODE_OFFSET`.
///
/// # Growth-only invariant
///
/// Entries are never removed from the slab. When a node disappears from
/// the L1 cache (e.g., swept by `sweep_stale_resolve`), its `InodeEntry`
/// remains here as a tombstone — the slab index (and thus the inode number)
/// is permanently consumed. This is acceptable because:
///
/// 1. **Inode stability:** FUSE clients may hold stale inode references.
///    Reusing an inode number for a different node would silently return
///    wrong data. The slab-based approach guarantees uniqueness.
/// 2. **Bounded growth:** The slab grows proportionally to the total
///    number of unique nodes ever discovered. For a FUSE mount scoped
///    to a single session, this is bounded by the project size.
/// 3. **`resolve_inode` safety:** The router validates that the L1 cache
///    still contains a matching entry before using an `InodeEntry`.
///    Stale entries are detected and return `None`.
pub(super) struct InodeMap {
    inner: RwLock<Slab<InodeEntry>>,
}

/// Default implementation for `InodeMap`.
impl Default for InodeMap {
    /// Delegates to [`InodeMap::new`].
    fn default() -> Self { Self::new() }
}

/// Inode allocation, lookup, and update operations.
impl InodeMap {
    /// Offset added to slab indices to produce inode numbers.
    /// Reserves 0 (unused by FUSE) and 1 (root directory).
    const INODE_OFFSET: u64 = 2;
    /// The root inode number (matches `FUSE_ROOT_INODE`).
    pub(super) const ROOT_INODE: u64 = 1;

    /// Create a new, empty inode map.
    pub(super) const fn new() -> Self {
        Self {
            inner: RwLock::new(Slab::new()),
        }
    }

    /// Allocate a new inode, returning its number.
    pub(super) fn allocate(&self, entry: InodeEntry) -> u64 {
        let mut slab = self.inner.write();
        let idx = slab.insert(entry);
        idx as u64 + Self::INODE_OFFSET
    }

    /// Look up an inode entry by number.
    pub(super) fn get(&self, inode: u64) -> Option<InodeEntry> {
        let idx = Self::inode_to_index(inode)?;
        let slab = self.inner.read();
        slab.get(idx).cloned()
    }

    /// Update the location fields of an existing inode.
    ///
    /// Used by `rename_node` to keep the inode map consistent after a
    /// filesystem rename — the inode number stays the same, but its
    /// location (directory, name, parent) changes.
    pub(super) fn update(&self, inode: u64, dir_path: VfsPath, name: String, parent_inode: u64) {
        let Some(idx) = Self::inode_to_index(inode) else {
            return;
        };
        let mut slab = self.inner.write();
        if let Some(entry) = slab.get_mut(idx) {
            entry.dir_path = dir_path;
            entry.name = name;
            entry.parent_inode = parent_inode;
        }
    }

    /// Convert an inode number to a slab index, returning `None` for
    /// reserved inodes (0 and `ROOT_INODE`) that aren't backed by the slab.
    fn inode_to_index(inode: u64) -> Option<usize> {
        inode
            .checked_sub(Self::INODE_OFFSET)
            .and_then(|i| usize::try_from(i).ok())
    }
}
