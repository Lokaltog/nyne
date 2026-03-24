//! Directory listing operations.

use super::{ReaddirEntry, Router};
use crate::types::ProcessVisibility;
use crate::types::vfs_path::VfsPath;

/// Directory listing operations for the router.
impl Router {
    /// Collect readdir entries for a resolved directory.
    ///
    /// Returns [`ReaddirEntry`] structs including `.` and `..`. The
    /// `visibility` level controls which nodes appear:
    /// - `All`: includes `Visibility::Hidden` nodes (e.g., companion dirs).
    /// - `Default`: only `Visibility::Readdir` nodes.
    /// - `None`: unreachable here — callers use `readdir_real` instead.
    ///
    /// Acquires and releases directory locks internally —
    /// callers never hold cache locks.
    pub(crate) fn collect_readdir_entries(
        &self,
        dir_path: &VfsPath,
        dir_inode: u64,
        visibility: ProcessVisibility,
    ) -> Vec<ReaddirEntry> {
        let mut entries = self.readdir_boilerplate(dir_inode);

        let Some(handle) = self.cache.get(dir_path) else {
            return entries;
        };

        let dir = handle.read();
        if dir.is_passthrough() {
            drop(dir); // Release lock before I/O.
            // Passthrough: no virtual content — read real entries directly.
            self.append_real_entries(&mut entries, dir_path);
        } else {
            for (name, cn) in dir.readdir_entries(visibility) {
                entries.push(ReaddirEntry {
                    inode: cn.inode,
                    kind: cn.kind.file_kind(),
                    name: name.to_owned(),
                });
            }
        }

        entries
    }

    /// Collect readdir entries from the real filesystem only — no providers.
    ///
    /// Used by passthrough processes (git, LSP servers) that must never see
    /// virtual nodes. Always reads real directory entries directly.
    pub(crate) fn readdir_real(&self, dir_path: &VfsPath, dir_inode: u64) -> Vec<ReaddirEntry> {
        let mut entries = self.readdir_boilerplate(dir_inode);
        self.append_real_entries(&mut entries, dir_path);
        entries
    }

    /// Resolve the `VfsPath` for a directory inode.
    ///
    /// For `ROOT_INODE` returns `VfsPath::root()`. For other inodes,
    /// reconstructs the path from the inode entry (`dir_path` + name).
    pub(crate) fn dir_path_for_inode(&self, inode: u64) -> Option<VfsPath> {
        if inode == Self::ROOT_INODE {
            return Some(VfsPath::root());
        }
        let entry = self.inodes.get(inode)?;
        entry.dir_path.join(&entry.name).ok()
    }

    /// Get the parent inode for a given inode.
    ///
    /// For `ROOT_INODE`, returns `ROOT_INODE` (root is its own parent).
    pub(crate) fn parent_inode(&self, inode: u64) -> u64 {
        if inode == Self::ROOT_INODE {
            return Self::ROOT_INODE;
        }
        self.inodes.get(inode).map_or(Self::ROOT_INODE, |e| e.parent_inode)
    }

    /// Assemble the `.` and `..` entries common to every readdir response.
    fn readdir_boilerplate(&self, dir_inode: u64) -> Vec<ReaddirEntry> {
        vec![
            ReaddirEntry::dot(dir_inode),
            ReaddirEntry::dotdot(self.parent_inode(dir_inode)),
        ]
    }

    /// Append real filesystem entries to a readdir result.
    ///
    /// Uses inode 0 as a hint; the kernel will call `lookup` for each
    /// entry it needs attributes for, which allocates lazily.
    fn append_real_entries(&self, entries: &mut Vec<ReaddirEntry>, dir_path: &VfsPath) {
        if let Ok(real_entries) = self.real_fs.read_dir(dir_path) {
            entries.extend(
                real_entries
                    .into_iter()
                    .map(|e| ReaddirEntry::real(0, e.file_type, e.name)),
            );
        }
    }
}
