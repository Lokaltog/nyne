//! Shared resolver for lazily accessing decomposed source files.
//!
//! This is the **single source of truth** for file identity + decomposition
//! cache. All content readers and splice writers hold a clone instead of
//! capturing `Arc<DecomposedSource>` snapshots, ensuring reads after writes
//! always see current content.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use color_eyre::eyre::Result;
use nyne::SymbolLineRange;

use crate::syntax;
use crate::syntax::decomposed::{DecomposedSource, DecompositionCache};

/// Lazy handle for accessing decomposed source files.
///
/// Holds a [`DecompositionCache`] reference and a source file path.
/// Every access re-decomposes (or hits the cache), guaranteeing fresh
/// data even after the source file has been modified.
///
/// This is the **canonical way** for content readers and splice writers to
/// access decomposition data. Capturing `Arc<DecomposedSource>` directly on
/// a `Readable`/`TemplateView` would snapshot stale data — `FragmentResolver`
/// defers resolution to call time so reads after writes see current content.
#[derive(Clone)]
pub struct FragmentResolver {
    cache: DecompositionCache,
    source_file: PathBuf,
}

impl FragmentResolver {
    /// Create a new resolver for the given source file.
    pub const fn new(cache: DecompositionCache, source_file: PathBuf) -> Self { Self { cache, source_file } }

    /// Get a fresh decomposition of the source file.
    pub fn decompose(&self) -> Result<Arc<DecomposedSource>> { self.cache.get(&self.source_file) }

    /// Evict cached decomposition, forcing re-parse on next access.
    pub fn invalidate(&self) { self.cache.invalidate(&self.source_file); }

    /// Return the source file path this resolver targets.
    pub fn source_file(&self) -> &Path { &self.source_file }

    /// Resolve the current line range for a symbol by fragment path.
    ///
    /// Always returns fresh data — decomposes at call time and navigates
    /// to the fragment. Returns `None` if the fragment path doesn't match
    /// any symbol in the current parse tree.
    pub fn line_range(&self, fragment_path: &[String]) -> Result<Option<SymbolLineRange>> {
        let decomposed = self.decompose()?;
        Ok(syntax::find_fragment(&decomposed.decomposed, fragment_path)
            .map(|f| SymbolLineRange::from_zero_based(&f.line_range(&decomposed.rope))))
    }
}
/// Inherent impl on [`DecompositionCache`] for building resolvers.
///
/// Lives here rather than alongside `DecompositionCache` in `syntax::decomposed`
/// to keep the `FragmentResolver` construction concern in one place.
impl DecompositionCache {
    /// Build a [`FragmentResolver`] bound to this cache and source file.
    ///
    /// Convenience over `FragmentResolver::new(cache.clone(), path)`.
    pub fn resolver(&self, source_file: PathBuf) -> FragmentResolver {
        FragmentResolver::new(self.clone(), source_file)
    }
}

#[cfg(test)]
mod tests;
