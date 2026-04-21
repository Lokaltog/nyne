use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use nyne::path_utils::PathExt;
use nyne::router::Filesystem;
use nyne::templates::TemplateHandle;
use nyne_source::{SourcePaths, SyntaxRegistry};
use parking_lot::RwLock;

use super::entry::Entry;
use super::scan::Scanner;
use crate::plugin::config::vfs::Vfs;

/// Shared state for the TODO plugin.
///
/// Created during plugin activation, shared between extension callbacks
/// (via captured `Arc`) and the [`TodoProvider`] (for invalidation).
pub struct TodoState {
    pub(crate) fs: Arc<dyn Filesystem>,
    pub(crate) syntax: Arc<SyntaxRegistry>,
    pub(crate) scanner: Scanner,
    pub(crate) index: RwLock<Option<Index>>,
    pub(crate) overview_tmpl: TemplateHandle,
    pub(crate) tag_tmpl: TemplateHandle,
    /// Canonical tag list from config (SSOT for priority order).
    pub(crate) tags: Vec<String>,
    /// Source plugin path builder for symlink targets.
    pub(crate) source_paths: Arc<SourcePaths>,
    /// Configured VFS path names.
    pub(crate) vfs: Vfs,
    /// Git repository handle for file discovery (when git feature is enabled).
    #[cfg(feature = "git")]
    pub(crate) repo: Option<Arc<nyne_git::Repo>>,
}
/// Cached scan results.
///
/// Built lazily on first access by scanning all git-tracked files.
/// Invalidated via `on_change` when any of the `scanned_files` is
/// modified, causing a full re-scan on the next read.
pub struct Index {
    /// All discovered entries, grouped by tag.
    pub(crate) entries_by_tag: BTreeMap<String, Vec<Entry>>,
    /// Set of files that were scanned (for invalidation).
    ///
    /// Uses `PathBuf` to match [`nyne::router::Provider::on_change`] parameter type.
    pub(crate) scanned_files: HashSet<PathBuf>,
}
impl TodoState {
    /// Ensure the index is populated, scanning lazily on first access.
    pub(crate) fn ensure_index(&self) {
        // Fast path: already populated.
        if self.index.read().is_some() {
            return;
        }

        let mut index = self.index.write();
        // Double-check after acquiring write lock — another thread may have
        // populated the index while we waited.
        if index.is_some() {
            return;
        }

        // Discover and scan inside the write lock to avoid redundant work
        // when multiple threads race past the read-lock fast path.
        let files = self.discover_files();
        let entries_by_tag = self.scanner.scan_all(&files, self.fs.as_ref(), &self.syntax);
        *index = Some(Index {
            entries_by_tag,
            scanned_files: files.into_iter().collect(),
        });
    }

    /// Get the list of files to scan from the git index.
    #[cfg(feature = "git")]
    fn discover_files(&self) -> Vec<PathBuf> {
        let Some(repo) = self.repo.as_ref() else {
            return Vec::new();
        };
        let Ok(paths) = repo.index_paths() else {
            return Vec::new();
        };

        let syntax = &self.syntax;
        paths
            .into_iter()
            .filter_map(|p| {
                let path = PathBuf::from(p);
                let ext = path.extension_str()?;
                // Only include files with a registered decomposer.
                syntax.get(ext)?;
                Some(path)
            })
            .collect()
    }

    /// Returns an empty list when the git feature is disabled.
    #[cfg(not(feature = "git"))]
    const fn discover_files(&self) -> Vec<PathBuf> { Vec::new() }
}
