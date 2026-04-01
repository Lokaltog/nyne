use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{PoisonError, RwLock};

/// Source file generation tracker. Bumped by fs watcher on change.
/// Shared across all providers via `Arc`.
///
/// Each source file has a monotonic generation counter. Caches compare
/// their stored generation against the current value to detect staleness.
/// Returns 0 for unknown files -- first access always triggers computation.
pub struct GenerationMap {
    inner: RwLock<HashMap<PathBuf, u64>>,
}

impl GenerationMap {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
        }
    }

    /// Get current generation for a file. Returns 0 for unknown files.
    pub fn get(&self, path: &Path) -> u64 {
        self.inner
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(path)
            .copied()
            .unwrap_or(0)
    }

    /// Bump generation for a file (called by fs watcher). Returns the new generation.
    pub fn bump(&self, path: &Path) -> u64 {
        let mut guard = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        let entry = guard.entry(path.to_owned()).or_insert(0);
        *entry += 1;
        *entry
    }
}

impl Default for GenerationMap {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests;
