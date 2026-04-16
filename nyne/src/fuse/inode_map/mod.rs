//! Bidirectional inode ↔ (dir_path, name) mapping for FUSE.
//!
//! Translates between FUSE inode numbers and filesystem paths.
//! Growth-only: entries are never removed to guarantee inode uniqueness.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use color_eyre::eyre::Result;
use parking_lot::RwLock;
use slab::Slab;

use crate::err::io_err;

/// Metadata for a single inode — its location in the directory tree.
#[derive(Clone, Debug)]
pub struct InodeEntry {
    pub dir_path: PathBuf,
    pub name: String,
    pub parent_inode: u64,
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
        self.full_path(inode)
            .ok_or_else(|| io_err(ErrorKind::NotFound, format!("inode {inode} not found")))
    }

    /// Get the parent inode for an inode, defaulting to [`ROOT_INODE`] if unknown.
    pub fn parent_of(&self, inode: u64) -> u64 {
        self.get(inode).map_or(ROOT_INODE, |e| e.parent_inode)
    }


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
            *slot = Arc::new(InodeEntry {
                dir_path,
                name,
                parent_inode,
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
