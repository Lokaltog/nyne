//! TODO/FIXME provider — scans source files for TODO markers.
//!
//! Provides shared [`TodoState`] (scanning, indexing, templates) and a
//! minimal [`TodoProvider`] for invalidation. Route dispatch is handled
//! by extension callbacks registered into
//! [`CompanionExtensions`](nyne_companion::CompanionExtensions)
//! during plugin activation — see [`routes::register_companion_extensions`].

/// TODO entry -- a single TODO marker in source code.
pub mod entry;
/// Extension registration for companion root routes.
pub mod routes;
/// TODO scanner -- Aho-Corasick automaton for tag detection.
pub mod scan;
pub mod state;
pub mod views;

use std::path::PathBuf;
use std::sync::Arc;

use nyne::prelude::*;
use nyne::router::InvalidationEvent;
use nyne_companion::CompanionProvider;
#[cfg(feature = "git")]
use nyne_git::GitProvider;
use nyne_source::SyntaxProvider;
pub use state::*;

/// TODO provider — handles invalidation when scanned files change.
///
/// Route dispatch is handled by extension callbacks registered into
/// [`CompanionExtensions`](nyne_companion::CompanionExtensions)
/// during plugin activation. This provider exists solely to participate
/// in the middleware chain for `on_change` invalidation.
pub struct TodoProvider {
    pub(crate) state: Arc<TodoState>,
}

#[cfg(feature = "git")]
nyne::define_provider!(TodoProvider, "todo", deps: [CompanionProvider, SyntaxProvider, GitProvider]);
#[cfg(not(feature = "git"))]
nyne::define_provider!(TodoProvider, "todo", deps: [CompanionProvider, SyntaxProvider]);

impl Provider for TodoProvider {
    fn on_change(&self, changed: &[PathBuf]) -> Vec<InvalidationEvent> {
        let index_guard = self.state.index.read();
        let Some(idx) = index_guard.as_ref() else {
            return Vec::new();
        };

        let dominated = changed.iter().any(|p| idx.scanned_files.contains(p));
        if !dominated {
            return Vec::new();
        }

        drop(index_guard);
        // Invalidate entire index — next access triggers rescan.
        *self.state.index.write() = None;
        vec![InvalidationEvent {
            path: PathBuf::from(&self.state.vfs.todo),
        }]
    }
}
