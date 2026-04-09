use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use color_eyre::eyre::{Result, bail};

use super::{DirEntry, Filesystem};
use crate::router::{AffectedFiles, NodeKind};
use crate::types::Timestamps;

/// In-memory node type.
#[derive(Debug, Clone)]
enum FsNode {
    File(Vec<u8>),
    Directory,
}

impl FsNode {
    const fn kind(&self) -> NodeKind {
        match self {
            Self::File(_) => NodeKind::File,
            Self::Directory => NodeKind::Directory,
        }
    }
}

/// In-memory filesystem for testing.
///
/// All paths are treated as absolute (no cwd tracking). Thread-safe via `RwLock`.
pub struct MemFs {
    nodes: RwLock<HashMap<PathBuf, FsNode>>,
}

impl MemFs {
    /// Create an empty filesystem with just the root directory.
    pub fn new() -> Self {
        let mut nodes = HashMap::new();
        nodes.insert(PathBuf::from("/"), FsNode::Directory);
        Self {
            nodes: RwLock::new(nodes),
        }
    }

    fn lock_read(&self) -> RwLockReadGuard<'_, HashMap<PathBuf, FsNode>> {
        self.nodes.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn lock_write(&self) -> RwLockWriteGuard<'_, HashMap<PathBuf, FsNode>> {
        self.nodes.write().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Default for MemFs {
    fn default() -> Self { Self::new() }
}

impl Filesystem for MemFs {
    fn source_dir(&self) -> &Path { Path::new("") }

    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let nodes = self.lock_read();
        match nodes.get(path) {
            Some(FsNode::Directory) => {}
            Some(FsNode::File(_)) => bail!("not a directory: {}", path.display()),
            None => bail!("directory not found: {}", path.display()),
        }
        let mut entries = Vec::new();
        for (child_path, node) in &*nodes {
            if child_path.parent() != Some(path) {
                continue;
            }
            let Some(file_name) = child_path.file_name() else {
                continue;
            };
            entries.push(DirEntry {
                name: file_name.to_string_lossy().to_string(),
                kind: node.kind(),
            });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    fn stat(&self, dir: &Path, name: &str) -> Result<Option<DirEntry>> {
        let nodes = self.lock_read();
        Ok(nodes.get(&dir.join(name)).map(|node| DirEntry {
            name: name.to_owned(),
            kind: node.kind(),
        }))
    }

    fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        match self.lock_read().get(path) {
            Some(FsNode::File(content)) => Ok(content.clone()),
            Some(FsNode::Directory) => bail!("is a directory: {}", path.display()),
            None => bail!("file not found: {}", path.display()),
        }
    }

    fn write_file(&self, path: &Path, content: &[u8]) -> Result<AffectedFiles> {
        let mut nodes = self.lock_write();
        if matches!(nodes.get(path), Some(FsNode::Directory)) {
            bail!("is a directory: {}", path.display());
        }
        nodes.insert(path.to_path_buf(), FsNode::File(content.to_vec()));
        Ok(vec![path.to_owned()])
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        let mut nodes = self.lock_write();
        let removed = nodes
            .remove(from)
            .ok_or_else(|| color_eyre::eyre::eyre!("not found: {}", from.display()))?;
        nodes.insert(to.to_path_buf(), removed);
        Ok(())
    }

    fn remove(&self, path: &Path) -> Result<()> {
        self.lock_write()
            .remove(path)
            .ok_or_else(|| color_eyre::eyre::eyre!("not found: {}", path.display()))?;
        Ok(())
    }

    fn create_file(&self, path: &Path) -> Result<()> {
        let mut nodes = self.lock_write();
        if nodes.contains_key(path) {
            bail!("already exists: {}", path.display());
        }
        nodes.insert(path.to_path_buf(), FsNode::File(Vec::new()));
        Ok(())
    }

    fn mkdir(&self, path: &Path) -> Result<()> {
        let mut nodes = self.lock_write();
        if nodes.contains_key(path) {
            bail!("already exists: {}", path.display());
        }
        nodes.insert(path.to_path_buf(), FsNode::Directory);
        Ok(())
    }

    fn metadata(&self, path: &Path) -> Result<super::Metadata> {
        match self.lock_read().get(path) {
            Some(node @ FsNode::File(data)) => Ok(super::Metadata {
                size: data.len() as u64,
                timestamps: Timestamps::default(),
                file_type: node.kind(),
                permissions: u32::from(super::mode::FILE_DEFAULT),
            }),
            Some(node @ FsNode::Directory) => Ok(super::Metadata {
                size: 0,
                timestamps: Timestamps::default(),
                file_type: node.kind(),
                permissions: u32::from(super::mode::DIR_DEFAULT),
            }),
            None => bail!("not found: {}", path.display()),
        }
    }

    fn symlink_target(&self, path: &Path) -> Result<PathBuf> { bail!("not a symlink: {}", path.display()) }
}
