//! Template handles — pre-bound template key + engine reference.
//!
//! [`TemplateHandle`] eliminates the per-call-site boilerplate of
//! `VirtualNode::file(name, TemplateContent::new(&engine, KEY, view))`.
//! Instead, providers call `handle.node(name, view)`.
//!
//! **Construction:** use [`HandleBuilder`] to register templates into a
//! shared engine, then call [`HandleBuilder::finish`] to get the
//! `Arc<TemplateEngine>` for constructing handles:
//!
//! ```ignore
//! let mut b = HandleBuilder::new();
//! let blame_key = b.register("git/blame", include_str!("templates/blame.md.j2"));
//! let log_key = b.register("git/log", include_str!("templates/log.md.j2"));
//! let engine = b.finish();
//! let blame = TemplateHandle::new(&engine, blame_key);
//! let log = TemplateHandle::new(&engine, log_key);
//! ```

use std::sync::Arc;

use color_eyre::eyre::Result;

use super::{TemplateContent, TemplateEngine, TemplateView};
use crate::node::VirtualNode;

/// A pre-bound template: engine reference + template key.
///
/// Produces [`VirtualNode`]s without requiring callers to know about
/// template keys or [`TemplateContent`] wiring.
pub struct TemplateHandle {
    engine: Arc<TemplateEngine>,
    key: &'static str,
}

/// Construction and rendering for pre-bound template handles.
impl TemplateHandle {
    /// Create a handle from a shared engine and a registered template key.
    pub fn new(engine: &Arc<TemplateEngine>, key: &'static str) -> Self {
        Self {
            engine: Arc::clone(engine),
            key,
        }
    }

    /// Create a [`VirtualNode::file`] backed by this template.
    pub fn node(&self, name: impl Into<String>, view: impl TemplateView + 'static) -> VirtualNode {
        VirtualNode::file(name, TemplateContent::new(&self.engine, self.key, view))
    }

    /// Render a view directly, returning the raw bytes.
    ///
    /// Useful in tests where you want to inspect template output without
    /// going through the FUSE read path.
    pub fn render_view(&self, view: &dyn TemplateView) -> Result<Vec<u8>> { view.render(&self.engine, self.key) }
}

/// Builder for registering templates into a shared engine.
///
/// Call [`register`](Self::register) for each template, then
/// [`finish`](Self::finish) to get the `Arc<TemplateEngine>`.
pub struct HandleBuilder {
    engine: TemplateEngine,
}

/// Default implementation for `HandleBuilder`.
impl Default for HandleBuilder {
    fn default() -> Self { Self::new() }
}

/// Template registration and engine construction.
impl HandleBuilder {
    pub fn new() -> Self {
        Self {
            engine: TemplateEngine::new(),
        }
    }

    /// Register a template and return its key (pass to [`TemplateHandle::new`]).
    pub fn register(&mut self, key: &'static str, source: &'static str) -> &'static str {
        self.engine.add_template(key, source);
        key
    }

    /// Register a partial template (included by other templates).
    ///
    /// Partials don't get their own handle — they're referenced via
    /// `{% include %}` in other templates.
    pub fn register_partial(&mut self, key: &'static str, source: &'static str) {
        self.engine.add_template(key, source);
    }

    /// Mutable access to the underlying engine (e.g. for registering globals).
    pub const fn engine_mut(&mut self) -> &mut TemplateEngine { &mut self.engine }

    /// Consume the builder and return the shared engine.
    pub fn finish(self) -> Arc<TemplateEngine> { Arc::new(self.engine) }
}
