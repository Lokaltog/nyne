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

use super::SyntaxRegistry;
use super::fragment::DecomposedFile;
use super::spec::Decomposer;

/// A source file decomposed into its constituent fragments.
///
/// Shared across providers that need access to a file's syntax structure.
/// All consumers should go through [`DecompositionCache::get`] rather than
/// calling `decompose_source` directly — the cache ensures each file is
/// parsed at most once per change cycle.
pub struct DecomposedSource {
    pub source: String,
    /// `crop::Rope` view of `source`, built once at parse time. Shared
    /// with every reader so byte→line conversions, line offsets, and
    /// splice operations don't have to rebuild a rope per call.
    /// `crop::Rope` is internally reference-counted, so cloning is cheap.
    pub rope: crop::Rope,
    pub decomposed: DecomposedFile,
    pub decomposer: Arc<dyn Decomposer>,
    /// The tree-sitter parse tree, retained for analysis.
    ///
    /// `None` for injection-based decomposers where the inner tree doesn't
    /// map to the outer source offsets.
    pub tree: Option<tree_sitter::Tree>,
}

/// Custom `Debug` that omits the raw source text and parse tree — tree-sitter's
/// `Tree` doesn't implement `Debug`, and the source string can be enormous.
impl fmt::Debug for DecomposedSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DecomposedSource")
            .field("decomposed", &self.decomposed)
            .finish_non_exhaustive()
    }
}

/// Read and decompose a source file, returning the shared decomposition.
///
/// Reads the file via the filesystem and delegates to
/// [`build_decomposed_source`] for the parse → map → resolve pipeline.
/// This is the single source of truth for "read file → decomposed
/// fragments" — all consumers must use this function rather than
/// calling decomposer methods directly.
fn decompose_source(
    source_path: &Path,
    fs: &dyn Filesystem,
    decomposer: &Arc<dyn Decomposer>,
    max_depth: usize,
) -> Result<Arc<DecomposedSource>> {
    Ok(build_decomposed_source(
        String::from_utf8(fs.read_file(source_path)?)?,
        Arc::clone(decomposer),
        max_depth,
    ))
}

/// Build a [`DecomposedSource`] from already-loaded source text.
///
/// Runs the parse → `assign_fs_names` → rope-build pipeline. This is the
/// single source of truth for assembling a `DecomposedSource` from a
/// `String` — both the cache loader and test helpers use it so the field
/// set never drifts.
pub fn build_decomposed_source(
    source: String,
    decomposer: Arc<dyn Decomposer>,
    max_depth: usize,
) -> Arc<DecomposedSource> {
    let (mut fragments, tree) = decomposer.decompose(&source, max_depth);
    decomposer.assign_fs_names(&mut fragments);
    Arc::new(DecomposedSource {
        rope: crop::Rope::from(source.as_str()),
        source,
        decomposed: fragments,
        decomposer,
        tree,
    })
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
