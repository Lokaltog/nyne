// Per-file diagnostic storage with condition-variable signaling.
//
// Captures `textDocument/publishDiagnostics` notifications from LSP servers
// and provides a blocking `get_or_wait` for read-time freshness. The dirty
// flag tracks whether a `didChange` was sent since the last publish — reads
// on clean files return immediately, reads on dirty files block until the
// server pushes fresh diagnostics or a timeout expires.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use lsp_types::Diagnostic;
use parking_lot::{Condvar, Mutex};

/// Per-file entry in the diagnostic store.
struct FileEntry {
    diagnostics: Vec<Diagnostic>,
    /// Whether a `didChange` was sent since the last `publishDiagnostics`.
    dirty: bool,
}

/// Stores push diagnostics from LSP servers and signals waiting readers.
///
/// Thread safety model:
/// - The reader thread (one per `LspClient`) calls [`publish`] when a
///   `textDocument/publishDiagnostics` notification arrives.
/// - FUSE threads (up to 4) call [`get_or_wait`] when reading DIAGNOSTICS.md.
/// - [`mark_dirty`] is called from `invalidate_file` after sending `didChange`.
///
/// The [`Condvar`] is signaled on every [`publish`], waking all waiting
/// FUSE threads. Each waiter re-checks its own file's dirty flag — only
/// the file that received the publish transitions to clean.
pub(crate) struct DiagnosticStore {
    files: Mutex<HashMap<PathBuf, FileEntry>>,
    notify: Condvar,
}

impl DiagnosticStore {
    /// Create a new empty diagnostic store.
    pub(crate) fn new() -> Self {
        Self {
            files: Mutex::new(HashMap::new()),
            notify: Condvar::new(),
        }
    }

    /// Mark a file as dirty after sending `didChange`.
    ///
    /// Subsequent [`get_or_wait`] calls for this file will block until
    /// [`publish`] clears the dirty flag.
    pub(crate) fn mark_dirty(&self, path: &Path) {
        self.files
            .lock()
            .entry(path.to_path_buf())
            .and_modify(|e| e.dirty = true)
            .or_insert(FileEntry {
                diagnostics: Vec::new(),
                dirty: true,
            });
    }

    /// Store diagnostics from a `publishDiagnostics` notification.
    ///
    /// Clears the dirty flag and wakes all waiting readers. Readers for
    /// other files will re-check their own dirty flag and loop back to wait.
    pub(crate) fn publish(&self, path: &Path, diagnostics: Vec<Diagnostic>) {
        {
            let mut files = self.files.lock();
            let entry = files.entry(path.to_path_buf()).or_insert(FileEntry {
                diagnostics: Vec::new(),
                dirty: false,
            });
            entry.diagnostics = diagnostics;
            entry.dirty = false;
        }
        // Signal outside the lock — waiters will re-acquire to check state.
        self.notify.notify_all();
    }

    /// Get diagnostics for a file, blocking if the file is dirty.
    ///
    /// - **Clean file**: returns immediately with stored diagnostics.
    /// - **Dirty file**: blocks until [`publish`] clears the dirty flag
    ///   or `timeout` expires, then returns whatever is stored (fresh
    ///   diagnostics on publish, stale/empty on timeout).
    /// - **Unknown file**: returns an empty vec immediately.
    pub(crate) fn get_or_wait(&self, path: &Path, timeout: Duration) -> Vec<Diagnostic> {
        let mut files = self.files.lock();

        let is_dirty = files.get(path).is_some_and(|e| e.dirty);
        if is_dirty {
            // wait_while_for: keeps waiting while the predicate returns true.
            // Predicate: "file is still dirty" → keep waiting.
            self.notify
                .wait_while_for(&mut files, |files| files.get(path).is_some_and(|e| e.dirty), timeout);
        }

        files.get(path).map(|e| e.diagnostics.clone()).unwrap_or_default()
    }

    /// Remove a file's entry entirely (e.g., on `didClose`).
    pub(crate) fn remove(&self, path: &Path) { self.files.lock().remove(path); }

    /// Signal that the server has refreshed diagnostics (pull model).
    ///
    /// Called when the server sends `workspace/diagnostic/refresh` — the
    /// pull-model equivalent of `publishDiagnostics`. Clears dirty flags
    /// on all files so blocked [`get_or_wait`] calls unblock and the
    /// caller can issue a fresh `textDocument/diagnostic` pull request.
    pub(crate) fn signal_refresh(&self) {
        {
            let mut files = self.files.lock();
            for entry in files.values_mut() {
                entry.dirty = false;
            }
        }
        self.notify.notify_all();
    }
}

#[cfg(test)]
mod tests;
