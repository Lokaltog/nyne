//! Bidirectional inode ↔ (dir_path, name) mapping for FUSE.
//!
//! Translates between FUSE inode numbers and filesystem paths.
//! Growth-only: entries are never removed to guarantee inode uniqueness.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use color_eyre::eyre::Result;
use parking_lot::RwLock;
use slab::Slab;

use crate::err;
use crate::router::{CachePolicy, NamedNode};

/// Metadata for a single inode — its location in the directory tree.
///
/// `node` + `expires_at` together form the "bound node" mechanism:
/// - `node = None` (the common case): the node is recoverable via the
///   provider chain on each lookup; the entry only carries location.
/// - `node = Some(_)` with `expires_at = Some(t)`: the node was attached
///   at allocation time (e.g. by an `on_create` callback that produces
///   nodes the chain doesn't otherwise surface). It stays addressable
///   via [`InodeMap::bound_node`] until `now > t`, then lazily clears.
/// - `node = Some(_)` with `expires_at = None`: bound for the inode's
///   full lifetime — reserved for callers that opt out of TTL.
#[derive(Clone, Debug)]
pub struct InodeEntry {
    pub dir_path: PathBuf,
    pub name: String,
    pub parent_inode: u64,
    pub node: Option<NamedNode>,
    pub expires_at: Option<Instant>,
}
impl InodeEntry {
    /// Construct an entry for a regular (chain-resolvable) inode.
    pub const fn new(dir_path: PathBuf, name: String, parent_inode: u64) -> Self {
        Self {
            dir_path,
            name,
            parent_inode,
            node: None,
            expires_at: None,
        }
    }
}

/// Bidirectional inode number ↔ location mapping.
///
/// Inode 0 is unused by FUSE, inode 1 is the root. Virtual inodes start
/// at offset 2. Backed by a [`Slab`] for O(1) inode→entry and a reverse
/// `HashMap` for O(1) (path, name)→inode. Entries are never removed
/// (growth-only) to guarantee inode uniqueness.
pub struct InodeMap {
    inner: RwLock<InodeMapInner>,
}

struct InodeMapInner {
    slab: Slab<Arc<InodeEntry>>,
    /// Reverse index: (`dir_path`, name) → inode number.
    by_path: HashMap<(PathBuf, String), u64>,
}

/// Offset added to slab indices to produce inode numbers.
const INODE_OFFSET: u64 = 2;

/// The root inode number (matches `FUSE_ROOT_INODE`).
pub(super) const ROOT_INODE: u64 = 1;

impl InodeMap {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(InodeMapInner {
                slab: Slab::new(),
                by_path: HashMap::new(),
            }),
        }
    }

    /// Allocate a new inode, returning its number.
    pub fn allocate(&self, entry: InodeEntry) -> u64 {
        let mut inner = self.inner.write();
        let ino_key = (entry.dir_path.clone(), entry.name.clone());
        let idx = inner.slab.insert(Arc::new(entry));
        let ino = idx as u64 + INODE_OFFSET;
        inner.by_path.insert(ino_key, ino);
        ino
    }

    /// Attach a node and TTL to an existing inode entry, making it
    /// reachable via [`Self::bound_node`] until the TTL elapses.
    ///
    /// Used by callbacks that produce nodes the chain cannot otherwise
    /// reproduce (e.g. `on_create` sinks). The TTL is sourced from the
    /// node's [`CachePolicy::Ttl`]; other policies leave the entry with
    /// no expiry (bound for the inode's full lifetime).
    pub fn bind_node(&self, inode: u64, node: NamedNode) {
        let Some(idx) = inode_to_index(inode) else {
            return;
        };
        let mut inner = self.inner.write();
        let Some(slot) = inner.slab.get_mut(idx) else {
            return;
        };
        let expires_at = match node.cache_policy() {
            CachePolicy::Ttl(d) => Some(Instant::now() + d),
            CachePolicy::Default | CachePolicy::NoCache => None,
        };
        *slot = Arc::new(InodeEntry {
            node: Some(node),
            expires_at,
            ..(**slot).clone()
        });
    }

    /// Return the bound node for `inode`, lazily evicting it if the TTL
    /// has elapsed.
    ///
    /// `None` is returned both for entries that were never bound and for
    /// entries whose binding has expired. Expired bindings are cleared
    /// in place — subsequent calls fall through to the chain dispatch.
    pub fn bound_node(&self, inode: u64) -> Option<NamedNode> {
        let idx = inode_to_index(inode)?;
        let mut inner = self.inner.write();
        let slot = inner.slab.get_mut(idx)?;
        let bound = slot.node.clone()?;
        if let Some(expires_at) = slot.expires_at
            && Instant::now() > expires_at
        {
            *slot = Arc::new(InodeEntry {
                node: None,
                expires_at: None,
                ..(**slot).clone()
            });
            return None;
        }
        Some(bound)
    }

    /// Refresh the TTL of a bound entry from its node's [`CachePolicy`].
    ///
    /// No-op for unbound entries, for entries whose node policy is not
    /// `Ttl`, or for unknown inodes. Called by the FUSE bridge after
    /// open/release to extend the binding's lifetime in lockstep with
    /// actual handle activity (stat / lookup do **not** extend).
    pub fn touch(&self, inode: u64) {
        let Some(idx) = inode_to_index(inode) else {
            return;
        };
        let mut inner = self.inner.write();
        let Some(slot) = inner.slab.get_mut(idx) else {
            return;
        };
        let Some(CachePolicy::Ttl(d)) = slot.node.as_ref().map(|n| n.cache_policy()) else {
            return;
        };
        *slot = Arc::new(InodeEntry {
            expires_at: Some(Instant::now() + d),
            ..(**slot).clone()
        });
    }

    /// Look up an inode entry by number.
    pub fn get(&self, inode: u64) -> Option<Arc<InodeEntry>> {
        let idx = inode_to_index(inode)?;
        self.inner.read().slab.get(idx).map(Arc::clone)
    }

    /// Get the directory path for an inode. For directory inodes, returns
    /// the path of the directory itself (`dir_path` + name).
    pub fn dir_path_for(&self, inode: u64) -> Option<PathBuf> {
        if inode == ROOT_INODE {
            return Some(PathBuf::new());
        }
        let entry = self.get(inode)?;
        Some(entry.dir_path.join(&entry.name))
    }

    /// Get the full path for an inode (`dir_path` + name).
    pub fn full_path(&self, inode: u64) -> Option<PathBuf> {
        let entry = self.get(inode)?;
        Some(entry.dir_path.join(&entry.name))
    }

    /// Resolve an inode to its full path, mapping a miss to
    /// [`ErrorKind::NotFound`] so the FUSE layer can surface `ENOENT`.
    pub fn resolve_path(&self, inode: u64) -> Result<PathBuf> {
        self.full_path(inode).ok_or_else(|| err::inode_not_found(inode))
    }

    /// Get the parent inode for an inode, defaulting to [`ROOT_INODE`] if unknown.
    pub fn parent_of(&self, inode: u64) -> u64 { self.get(inode).map_or(ROOT_INODE, |e| e.parent_inode) }

    /// Update location after a rename.
    pub fn update(&self, inode: u64, dir_path: PathBuf, name: String, parent_inode: u64) -> bool {
        let Some(idx) = inode_to_index(inode) else {
            return false;
        };
        let mut inner = self.inner.write();
        let Some(old) = inner.slab.get(idx).map(Arc::clone) else {
            return false;
        };
        // Remove old reverse index entry before replacing.
        inner.by_path.remove(&(old.dir_path.clone(), old.name.clone()));
        let ino = idx as u64 + INODE_OFFSET;
        inner.by_path.insert((dir_path.clone(), name.clone()), ino);
        if let Some(slot) = inner.slab.get_mut(idx) {
            // Preserve any bound node + expiry — rename moves the entry,
            // not its lifetime.
            let prev = Arc::clone(slot);
            *slot = Arc::new(InodeEntry {
                dir_path,
                name,
                parent_inode,
                node: prev.node.clone(),
                expires_at: prev.expires_at,
            });
        }
        true
    }

    /// Find the inode for a (`dir_path`, name) pair via reverse index.
    pub fn find_inode(&self, dir_path: &Path, name: &str) -> Option<u64> {
        self.inner
            .read()
            .by_path
            .get(&(dir_path.to_path_buf(), name.to_owned()))
            .copied()
    }
}

fn inode_to_index(inode: u64) -> Option<usize> { inode.checked_sub(INODE_OFFSET).and_then(|i| usize::try_from(i).ok()) }

#[cfg(test)]
mod tests;
