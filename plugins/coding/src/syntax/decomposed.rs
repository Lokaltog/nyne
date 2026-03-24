//! Shared decomposition cache and source file decomposition results.
//!
//! [`DecomposedSource`] holds a parsed file with its decomposed fragments,
//! and the tree-sitter parse tree for analysis. [`DecompositionCache`] ensures
//! each file is parsed at most once per change cycle.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use color_eyre::eyre::Result;
use nyne::types::real_fs::RealFs;
use nyne::types::vfs_path::VfsPath;
use parking_lot::RwLock;

use super::fragment::{DEFAULT_MAX_DEPTH, DecomposedFile};
use super::spec::Decomposer;
use super::{SyntaxRegistry, resolve_conflicts};

/// A source file decomposed into its constituent fragments.
///
/// Shared across providers that need access to a file's syntax structure.
/// All consumers should go through [`DecompositionCache::get`] rather than
/// calling `decompose_source` directly — the cache ensures each file is
/// parsed at most once per change cycle.
pub struct DecomposedSource {
    pub source: String,
    pub decomposed: DecomposedFile,
    pub decomposer: Arc<dyn Decomposer>,
    /// The tree-sitter parse tree, retained for analysis.
    ///
    /// `None` for injection-based decomposers where the inner tree doesn't
    /// map to the outer source offsets.
    pub tree: Option<tree_sitter::Tree>,
}

/// Debug implementation for `DecomposedSource`, showing only the decomposed fragments.
impl fmt::Debug for DecomposedSource {
    /// Formats the decomposed source for debug output.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DecomposedSource")
            .field("decomposed", &self.decomposed)
            .finish_non_exhaustive()
    }
}

/// Read and decompose a source file, returning the shared decomposition.
///
/// Reads the file via `real_fs`, parses with the given decomposer, maps
/// filesystem names, and resolves conflicts. This is the single source of
/// truth for "read file → decomposed fragments" — all consumers must use
/// this function rather than calling decomposer methods directly.
fn decompose_source(
    source_path: &VfsPath,
    real_fs: &dyn RealFs,
    decomposer: &Arc<dyn Decomposer>,
) -> Result<Arc<DecomposedSource>> {
    let raw = real_fs.read(source_path)?;
    let source = String::from_utf8(raw)?;
    let (mut fragments, tree) = decomposer.decompose(&source, DEFAULT_MAX_DEPTH);
    decomposer.map_to_fs(&mut fragments);
    resolve_conflicts(&mut fragments, decomposer);
    Ok(Arc::new(DecomposedSource {
        source,
        decomposed: fragments,
        decomposer: Arc::clone(decomposer),
        tree,
    }))
}

/// Per-file decomposition cache.
///
/// Owns the `RealFs` used to read source files and the `SyntaxRegistry`
/// used to look up decomposers. Shared via `ActivationContext` so that
/// all providers use a single cache — one decomposition per file per
/// change cycle.
///
/// Cloning is cheap (`Arc` bump) — pass freely to node capabilities
/// that need cache access for invalidation after writes.
#[derive(Clone)]
pub struct DecompositionCache {
    inner: Arc<DecompositionCacheInner>,
}

/// Inner state of the decomposition cache behind `Arc`.
struct DecompositionCacheInner {
    real_fs: Arc<dyn RealFs>,
    syntax: Arc<SyntaxRegistry>,
    cache: RwLock<HashMap<VfsPath, Arc<DecomposedSource>>>,
}

/// Cache management: creation, lookup, and invalidation.
impl DecompositionCache {
    /// Create a new empty cache.
    pub(crate) fn new(real_fs: Arc<dyn RealFs>, syntax: Arc<SyntaxRegistry>) -> Self {
        Self {
            inner: Arc::new(DecompositionCacheInner {
                real_fs,
                syntax,
                cache: RwLock::new(HashMap::new()),
            }),
        }
    }

    /// Get or compute the decomposed source for a file.
    ///
    /// Returns the cached result if available, otherwise reads and
    /// decomposes the file, caches the result, and returns it.
    pub fn get(&self, source_file: &VfsPath) -> Result<Arc<DecomposedSource>> {
        if let Some(entry) = self.inner.cache.read().get(source_file) {
            return Ok(Arc::clone(entry));
        }
        let decomposer = self
            .inner
            .syntax
            .decomposer_for(source_file)
            .ok_or_else(|| color_eyre::eyre::eyre!("no decomposer for {source_file}"))?;
        let shared = decompose_source(source_file, &*self.inner.real_fs, decomposer)?;
        self.inner
            .cache
            .write()
            .insert(source_file.clone(), Arc::clone(&shared));
        Ok(shared)
    }

    /// Evict cached decomposition for a file.
    ///
    /// Called on filesystem changes to ensure the next `get()` re-reads
    /// and re-decomposes the file.
    pub fn invalidate(&self, path: &VfsPath) { self.inner.cache.write().remove(path); }
}
