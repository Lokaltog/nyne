//! Template handles — pre-bound template key + engine reference.
//!
//! [`TemplateHandle`] eliminates the per-call-site boilerplate of
//! `Node::file().with_readable(TemplateContent::new(&engine, KEY, view)).named(name)`.
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

use color_eyre::eyre;

use super::TemplateEngine;
use crate::prelude::*;
use crate::router::{NamedNode, Node, ReadContext, Readable, Writable, WriteContext};

/// A pre-bound template: engine reference + template key.
///
/// Produces named nodes without requiring callers to know about
/// template keys or [`TemplateContent`] wiring.
#[derive(Clone)]
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

    /// Render a view directly, returning the raw bytes.
    ///
    /// Useful in tests where you want to inspect template output without
    /// going through the FUSE read path.
    pub fn render_view(&self, view: &dyn TemplateView) -> Result<Vec<u8>> { view.render(&self.engine, self.key) }

    /// Create a [`NamedNode`] file backed by this template.
    ///
    /// The view is rendered lazily at read time so content is never stale.
    pub fn named_node(&self, name: impl Into<String>, view: impl TemplateView + 'static) -> NamedNode {
        Node::file()
            .with_readable(RouterTemplateReadable {
                engine: Arc::clone(&self.engine),
                key: self.key,
                view: Box::new(view),
            })
            .named(name)
    }

    /// Create an editable [`NamedNode`] backed by this template.
    ///
    /// Like [`named_node`](Self::named_node), but the view also handles writes.
    /// Pass an `Arc` so the same view backs both the readable and writable slots.
    pub fn editable_named_node(
        &self,
        name: impl Into<String>,
        view: Arc<impl TemplateView + Writable + 'static>,
    ) -> NamedNode {
        Node::file()
            .with_readable(RouterTemplateReadable {
                engine: Arc::clone(&self.engine),
                key: self.key,
                view: Box::new(Arc::clone(&view)),
            })
            .with_writable(view)
            .named(name)
    }

    /// Create a closure-backed template node.
    ///
    /// Convenience wrapper around [`named_node`](Self::named_node) +
    /// [`LazyView`](super::LazyView) — callers provide only the render closure.
    pub fn lazy_node(
        &self,
        name: impl Into<String>,
        read_fn: impl Fn(&TemplateEngine, &str) -> Result<Vec<u8>> + Send + Sync + 'static,
    ) -> NamedNode {
        self.named_node(name, super::LazyView::new(read_fn))
    }

    /// Create an editable closure-backed template node.
    ///
    /// Like [`lazy_node`](Self::lazy_node), but also attaches a write callback.
    pub fn editable_lazy_node(
        &self,
        name: impl Into<String>,
        read_fn: impl Fn(&TemplateEngine, &str) -> Result<Vec<u8>> + Send + Sync + 'static,
        write_fn: impl Fn(&WriteContext<'_>, &[u8]) -> Result<AffectedFiles> + Send + Sync + 'static,
    ) -> NamedNode {
        self.editable_named_node(name, Arc::new(super::LazyView::new(read_fn).writable(write_fn)))
    }
}
/// Lazy template renderer implementing [`crate::router::Readable`].
///
/// Renders the template at read time so content is never stale.
/// Created by [`TemplateHandle::named_node`].
struct RouterTemplateReadable {
    engine: Arc<TemplateEngine>,
    key: &'static str,
    view: Box<dyn super::TemplateView>,
}

impl Readable for RouterTemplateReadable {
    fn read(&self, _ctx: &ReadContext<'_>) -> eyre::Result<Vec<u8>> { self.view.render(&self.engine, self.key) }
}

/// Builder for registering templates into a shared engine.
///
/// Call [`register`](Self::register) for each template, then
/// [`finish`](Self::finish) to get the `Arc<TemplateEngine>`.
#[derive(Default)]
pub struct HandleBuilder {
    engine: TemplateEngine,
}

/// Template registration and engine construction.
impl HandleBuilder {
    /// Creates a new handle builder with a default template engine.
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
