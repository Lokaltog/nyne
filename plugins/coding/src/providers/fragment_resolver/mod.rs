//! Shared resolver for lazily accessing decomposed source files.
//!
//! This is the **single source of truth** for file identity + decomposition
//! cache. All content readers and splice writers hold a clone instead of
//! capturing `Arc<DecomposedSource>` snapshots, ensuring reads after writes
//! always see current content.

use std::sync::Arc;

use color_eyre::eyre::Result;
use nyne::types::SymbolLineRange;
use nyne::types::vfs_path::VfsPath;

use crate::syntax;
use crate::syntax::decomposed::{DecomposedSource, DecompositionCache};

/// Lazy handle for accessing decomposed source files.
///
/// Holds a [`DecompositionCache`] reference and a source file path.
/// Every access re-decomposes (or hits the cache), guaranteeing fresh
/// data even after the source file has been modified.
#[derive(Clone)]
pub struct FragmentResolver {
    cache: DecompositionCache,
    source_file: VfsPath,
}

impl FragmentResolver {
    pub const fn new(cache: DecompositionCache, source_file: VfsPath) -> Self { Self { cache, source_file } }

    /// Get a fresh decomposition of the source file.
    pub fn decompose(&self) -> Result<Arc<DecomposedSource>> { self.cache.get(&self.source_file) }

    /// Evict cached decomposition, forcing re-parse on next access.
    pub fn invalidate(&self) { self.cache.invalidate(&self.source_file); }

    pub const fn source_file(&self) -> &VfsPath { &self.source_file }

    /// Resolve the current line range for a symbol by fragment path.
    ///
    /// Always returns fresh data — decomposes at call time and navigates
    /// to the fragment. Returns `None` if the fragment path doesn't match
    /// any symbol in the current parse tree.
    pub fn line_range(&self, fragment_path: &[String]) -> Result<Option<SymbolLineRange>> {
        let decomposed = self.decompose()?;
        let range = syntax::find_fragment(&decomposed.decomposed, fragment_path)
            .map(|f| SymbolLineRange::from_zero_based(&f.line_range(&decomposed.source)));
        Ok(range)
    }
}

#[cfg(test)]
mod tests;
