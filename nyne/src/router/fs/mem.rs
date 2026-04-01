use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{PoisonError, RwLock};

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

    /// Seed a file with content. Creates parent directories automatically.
    pub fn add_file(&self, path: impl AsRef<Path>, content: impl AsRef<[u8]>) {
        let path = path.as_ref().to_path_buf();
        let mut nodes = self.nodes.write().unwrap_or_else(PoisonError::into_inner);
        for ancestor in path.ancestors().skip(1) {
            nodes.entry(ancestor.to_path_buf()).or_insert(FsNode::Directory);
        }
        nodes.insert(path, FsNode::File(content.as_ref().to_vec()));
    }

    /// Seed an empty directory. Creates parent directories automatically.
    pub fn add_dir(&self, path: impl AsRef<Path>) {
        let path = path.as_ref().to_path_buf();
        let mut nodes = self.nodes.write().unwrap_or_else(PoisonError::into_inner);
        for ancestor in path.ancestors() {
            nodes.entry(ancestor.to_path_buf()).or_insert(FsNode::Directory);
        }
    }

    /// Read current file content (for test assertions).
    pub fn content(&self, path: impl AsRef<Path>) -> Option<Vec<u8>> {
        let nodes = self.nodes.read().unwrap_or_else(PoisonError::into_inner);
        match nodes.get(path.as_ref()) {
            Some(FsNode::File(content)) => Some(content.clone()),
            _ => None,
        }
    }

    /// Check if a path exists (for test assertions).
    pub fn exists(&self, path: impl AsRef<Path>) -> bool {
        self.nodes
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(path.as_ref())
    }

    /// List all paths in the filesystem (for debugging/assertions).
    pub fn all_paths(&self) -> Vec<PathBuf> {
        let mut paths: Vec<_> = self
            .nodes
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .keys()
            .cloned()
            .collect();
        paths.sort();
        paths
    }
}

impl Default for MemFs {
    fn default() -> Self { Self::new() }
}

impl Filesystem for MemFs {
    fn source_dir(&self) -> &Path { Path::new("") }

    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let nodes = self.nodes.read().unwrap_or_else(PoisonError::into_inner);
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
            let kind = match node {
                FsNode::File(_) => NodeKind::File,
                FsNode::Directory => NodeKind::Directory,
            };
            entries.push(DirEntry {
                name: file_name.to_string_lossy().to_string(),
                kind,
            });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    fn stat(&self, dir: &Path, name: &str) -> Result<Option<DirEntry>> {
        let nodes = self.nodes.read().unwrap_or_else(PoisonError::into_inner);
        let path = dir.join(name);
        Ok(match nodes.get(&path) {
            Some(FsNode::File(_)) => Some(DirEntry {
                name: name.to_owned(),
                kind: NodeKind::File,
            }),
            Some(FsNode::Directory) => Some(DirEntry {
                name: name.to_owned(),
                kind: NodeKind::Directory,
            }),
            None => None,
        })
    }

    fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        let nodes = self.nodes.read().unwrap_or_else(PoisonError::into_inner);
        match nodes.get(path) {
            Some(FsNode::File(content)) => Ok(content.clone()),
            Some(FsNode::Directory) => bail!("is a directory: {}", path.display()),
            None => bail!("file not found: {}", path.display()),
        }
    }

    fn write_file(&self, path: &Path, content: &[u8]) -> Result<AffectedFiles> {
        let mut nodes = self.nodes.write().unwrap_or_else(PoisonError::into_inner);
        if matches!(nodes.get(path), Some(FsNode::Directory)) {
            bail!("is a directory: {}", path.display());
        }
        nodes.insert(path.to_path_buf(), FsNode::File(content.to_vec()));
        Ok(vec![path.to_owned()])
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        let mut nodes = self.nodes.write().unwrap_or_else(PoisonError::into_inner);
        let node = nodes
            .remove(from)
            .ok_or_else(|| color_eyre::eyre::eyre!("not found: {}", from.display()))?;
        nodes.insert(to.to_path_buf(), node);
        Ok(())
    }

    fn remove(&self, path: &Path) -> Result<()> {
        let mut nodes = self.nodes.write().unwrap_or_else(PoisonError::into_inner);
        nodes
            .remove(path)
            .ok_or_else(|| color_eyre::eyre::eyre!("not found: {}", path.display()))?;
        Ok(())
    }

    fn create_file(&self, path: &Path) -> Result<()> {
        let mut nodes = self.nodes.write().unwrap_or_else(PoisonError::into_inner);
        if nodes.contains_key(path) {
            bail!("already exists: {}", path.display());
        }
        nodes.insert(path.to_path_buf(), FsNode::File(Vec::new()));
        Ok(())
    }

    fn mkdir(&self, path: &Path) -> Result<()> {
        let mut nodes = self.nodes.write().unwrap_or_else(PoisonError::into_inner);
        if nodes.contains_key(path) {
            bail!("already exists: {}", path.display());
        }
        nodes.insert(path.to_path_buf(), FsNode::Directory);
        Ok(())
    }

    fn metadata(&self, path: &Path) -> Result<super::Metadata> {
        let nodes = self.nodes.read().unwrap_or_else(PoisonError::into_inner);
        match nodes.get(path) {
            Some(FsNode::File(data)) => Ok(super::Metadata {
                size: data.len() as u64,
                timestamps: Timestamps::default(),
                file_type: NodeKind::File,
                permissions: 0o644,
            }),
            Some(FsNode::Directory) => Ok(super::Metadata {
                size: 0,
                timestamps: Timestamps::default(),
                file_type: NodeKind::Directory,
                permissions: 0o755,
            }),
            None => bail!("not found: {}", path.display()),
        }
    }

    fn symlink_target(&self, path: &Path) -> Result<PathBuf> { bail!("not a symlink: {}", path.display()) }
}
