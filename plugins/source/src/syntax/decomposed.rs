//! Shared decomposition cache and source file decomposition results.
//!
//! [`DecomposedSource`] holds a parsed file with its decomposed fragments,
//! and the tree-sitter parse tree for analysis. [`DecompositionCache`] ensures
//! each file is parsed at most once per change cycle.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use color_eyre::eyre::Result;
use nyne::router::Filesystem;
use parking_lot::RwLock;

use super::fragment::DecomposedFile;
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

/// Custom `Debug` that omits the raw source text and parse tree.
///
/// The full source string can be enormous and the tree-sitter `Tree` type
/// does not implement `Debug`, so this focuses on the fragment structure
/// which is what matters when diagnosing decomposition issues.
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
/// Reads the file via the filesystem, parses with the given decomposer, maps
/// filesystem names, and resolves conflicts. This is the single source of
/// truth for "read file → decomposed fragments" — all consumers must use
/// this function rather than calling decomposer methods directly.
fn decompose_source(
    source_path: &Path,
    fs: &dyn Filesystem,
    decomposer: &Arc<dyn Decomposer>,
    max_depth: usize,
) -> Result<Arc<DecomposedSource>> {
    let raw = fs.read_file(source_path)?;
    let source = String::from_utf8(raw)?;
    let (mut fragments, tree) = decomposer.decompose(&source, max_depth);
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
/// Owns the [`Filesystem`] used to read source files and the `SyntaxRegistry`
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
///
/// Separated from [`DecompositionCache`] so that cloning the cache is a
/// cheap `Arc` bump without requiring `RwLock<HashMap<...>>` to be `Clone`.
struct DecompositionCacheInner {
    fs: Arc<dyn Filesystem>,
    syntax: Arc<SyntaxRegistry>,
    max_depth: usize,
    cache: RwLock<HashMap<PathBuf, Arc<DecomposedSource>>>,
}

/// Cache management: creation, lookup, and invalidation.
impl DecompositionCache {
    /// Create a new empty cache.
    pub(crate) fn new(fs: Arc<dyn Filesystem>, syntax: Arc<SyntaxRegistry>, max_depth: usize) -> Self {
        Self {
            inner: Arc::new(DecompositionCacheInner {
                fs,
                syntax,
                max_depth,
                cache: RwLock::new(HashMap::new()),
            }),
        }
    }

    /// Get or compute the decomposed source for a file.
    ///
    /// Returns the cached result if available, otherwise reads and
    /// decomposes the file, caches the result, and returns it.
    ///
    /// The expensive decomposition runs without any lock held so that
    /// concurrent readers are never blocked.  A re-check under the
    /// write lock prevents redundant insertions when two threads race
    /// past the initial read-lock check simultaneously.
    pub fn get(&self, source_file: &Path) -> Result<Arc<DecomposedSource>> {
        // Fast path: return cached entry under a read lock.
        if let Some(entry) = self.inner.cache.read().get(source_file) {
            return Ok(Arc::clone(entry));
        }

        // Decompose without any lock held — this is the expensive part.
        let shared = decompose_source(
            source_file,
            self.inner.fs.as_ref(),
            self.inner
                .syntax
                .decomposer_for(source_file)
                .ok_or_else(|| color_eyre::eyre::eyre!("no decomposer for {}", source_file.display()))?,
            self.inner.max_depth,
        )?;

        // Slow path: re-check under write lock — another thread may have
        // inserted while we were decomposing.
        let mut cache = self.inner.cache.write();
        if let Some(entry) = cache.get(source_file) {
            return Ok(Arc::clone(entry));
        }
        cache.insert(source_file.to_path_buf(), Arc::clone(&shared));
        Ok(shared)
    }

    /// Check whether a fragment exists at `path` in the decomposition for `source_file`.
    ///
    /// Lightweight check used by `edit/` sub-routes to validate a fragment
    /// before emitting nodes — avoids full directory resolution.
    pub fn has_fragment(&self, source_file: &Path, fragment_path: &[String]) -> bool {
        let Ok(shared) = self.get(source_file) else {
            return false;
        };
        super::find_fragment(&shared.decomposed, fragment_path).is_some()
    }

    /// Evict cached decomposition for a file.
    ///
    /// Called on filesystem changes to ensure the next `get()` re-reads
    /// and re-decomposes the file.
    pub fn invalidate(&self, path: &Path) { self.inner.cache.write().remove(path); }
}
