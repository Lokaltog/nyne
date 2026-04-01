//! Provider-agnostic route extensions for cross-plugin tree composition.
//!
//! [`RouteExtension`] allows plugins to register readdir, lookup, content,
//! and handler callbacks during activation. The owning plugin applies them
//! via [`TreeBuilder::apply`], producing a unified route tree without
//! cross-plugin name coupling.

use std::sync::Arc;

use color_eyre::eyre::Result;
use tracing::debug;

use super::NamedNode;
use super::chain::Next;
use super::request::Request;
use super::route::RouteCtx;
use super::tree::Entry;

// Provider-agnostic callback signatures. Arc-wrapped so `TreeBuilder::apply`
// can clone them into typed closures that ignore the `&T` provider reference.
type ReaddirCb = Arc<dyn Fn(&RouteCtx, &mut Request) -> Result<()> + Send + Sync>;
type LookupCb = Arc<dyn Fn(&RouteCtx, &mut Request, &str) -> Result<()> + Send + Sync>;
type ContentCb = Arc<dyn Fn(&RouteCtx, &Request) -> Option<NamedNode> + Send + Sync>;
type HandlerCb = Arc<dyn for<'a> Fn(&RouteCtx, &mut Request, &Next<'a>) -> Result<()> + Send + Sync>;

/// Provider-agnostic route extension — callbacks mountable into any provider's tree.
///
/// Mirrors the [`TreeBuilder`](super::tree::TreeBuilder) registration API
/// (readdir, lookup, content, handler, dir, capture, rest) without a provider
/// type parameter. When [`TreeBuilder::apply`] is called, each callback is
/// wrapped in a closure that ignores the `&T` provider reference — extensions
/// carry their own state via captured `Arc`s.
///
/// # Lifecycle
///
/// 1. The owning plugin inserts a struct containing `RouteExtension` fields
///    into [`ActivationContext`] during `activate()`.
/// 2. Downstream plugins call `ctx.get_or_insert_default::<Extensions>()` in
///    their `activate()` and register callbacks.
/// 3. The owning plugin reads the extensions in `providers()` and calls
///    `builder.apply(&ext)` at the appropriate tree level.
///
/// [`ActivationContext`]: crate::dispatch::activation::ActivationContext
#[derive(Default)]
pub struct RouteExtension {
    on_readdir: Vec<ReaddirCb>,
    on_lookup: Vec<LookupCb>,
    handler: Option<HandlerCb>,
    entries: Vec<Entry<ContentCb, Self>>,
}

impl RouteExtension {
    /// Create an empty extension.
    pub fn new() -> Self { Self::default() }

    /// Whether this extension has any registered callbacks or entries.
    pub fn is_empty(&self) -> bool {
        self.on_readdir.is_empty() && self.on_lookup.is_empty() && self.handler.is_none() && self.entries.is_empty()
    }

    /// Register a readdir callback (additive — multiple allowed).
    pub fn on_readdir(&mut self, f: impl Fn(&RouteCtx, &mut Request) -> Result<()> + Send + Sync + 'static) {
        self.on_readdir.push(Arc::new(f));
    }

    /// Register a lookup callback (additive — multiple allowed).
    pub fn on_lookup(&mut self, f: impl Fn(&RouteCtx, &mut Request, &str) -> Result<()> + Send + Sync + 'static) {
        self.on_lookup.push(Arc::new(f));
    }

    /// Register a content producer (additive — multiple allowed).
    pub fn content(&mut self, f: impl Fn(&RouteCtx, &Request) -> Option<NamedNode> + Send + Sync + 'static) {
        self.entries.push(Entry::Content {
            content_fn: Arc::new(f),
        });
    }

    /// Register an infallible content producer (additive — multiple allowed).
    ///
    /// Same as [`content`](Self::content) but the factory returns `NamedNode`
    /// directly — internally wrapped in `Some`.
    pub fn content_always(&mut self, f: impl Fn(&RouteCtx, &Request) -> NamedNode + Send + Sync + 'static) {
        self.entries.push(Entry::Content {
            content_fn: Arc::new(move |ctx, req| Some(f(ctx, req))),
        });
    }

    /// Register a handler (singular — replaces any previous).
    pub fn handler(
        &mut self,
        f: impl for<'a> Fn(&RouteCtx, &mut Request, &Next<'a>) -> Result<()> + Send + Sync + 'static,
    ) {
        self.handler = Some(Arc::new(f));
    }

    /// Register a static named subdirectory with a subtree.
    pub fn dir(&mut self, name: impl Into<String>, f: impl FnOnce(&mut Self)) {
        let mut sub = Self::new();
        f(&mut sub);
        self.entries.push(Entry::Dir {
            name: name.into(),
            tree: sub,
        });
    }

    /// Register a dynamic single-segment capture with a subtree.
    pub fn capture(&mut self, param: &'static str, f: impl FnOnce(&mut Self)) {
        let mut sub = Self::new();
        f(&mut sub);
        self.entries.push(Entry::Capture { param, tree: sub });
    }

    /// Register a dynamic rest capture (1+ segments) with a subtree.
    pub fn rest(&mut self, param: &'static str, f: impl FnOnce(&mut Self)) {
        let mut sub = Self::new();
        f(&mut sub);
        self.entries.push(Entry::Rest { param, tree: sub });
    }

    /// Register callbacks within a named scope for provenance tracking.
    ///
    /// All entries registered within the closure are tagged with the scope
    /// name and logged at `debug` level. The entries are then merged into
    /// this extension.
    pub fn scoped(&mut self, scope: impl Into<String>, f: impl FnOnce(&mut Self)) {
        let mut inner = Self::new();
        f(&mut inner);
        let scope = scope.into();
        debug!(
            scope,
            entries = inner.entries.len(),
            on_readdir = inner.on_readdir.len(),
            on_lookup = inner.on_lookup.len(),
            has_handler = inner.handler.is_some(),
            "registered extension callbacks",
        );
        self.merge(inner);
    }

    /// Apply all registered callbacks onto a typed [`TreeBuilder`].
    ///
    /// Called by [`TreeBuilder::apply`] — prefer that method for chaining.
    /// Generic over `T`: each callback is wrapped in a closure that ignores
    /// the `&T` provider reference.
    pub(crate) fn apply_to<T: Send + Sync + 'static>(
        &self,
        mut d: super::tree::TreeBuilder<T>,
    ) -> super::tree::TreeBuilder<T> {
        for cb in &self.on_readdir {
            let cb = Arc::clone(cb);
            d = d.on_readdir(move |_p, ctx, req| cb(ctx, req));
        }
        for cb in &self.on_lookup {
            let cb = Arc::clone(cb);
            d = d.on_lookup(move |_p, ctx, req, name| cb(ctx, req, name));
        }
        if let Some(cb) = &self.handler {
            let cb = Arc::clone(cb);
            d = d.handler(move |_p, ctx, req, next| cb(ctx, req, next));
        }
        for entry in &self.entries {
            match entry {
                Entry::Content { content_fn } => {
                    let cb = Arc::clone(content_fn);
                    d = d.content(move |_p, ctx, req| cb(ctx, req));
                }
                Entry::Dir { name, tree } => {
                    d = d.dir(name.clone(), |d| tree.apply_to(d));
                }
                Entry::Capture { param, tree } => {
                    d = d.capture(param, |d| tree.apply_to(d));
                }
                Entry::Rest { param, tree } => {
                    d = d.rest(param, |d| tree.apply_to(d));
                }
            }
        }
        d
    }

    /// Merge another extension's callbacks and entries into this one.
    pub fn merge(&mut self, other: Self) {
        self.on_readdir.extend(other.on_readdir);
        self.on_lookup.extend(other.on_lookup);
        self.entries.extend(other.entries);
        if other.handler.is_some() {
            self.handler = other.handler;
        }
    }
}

#[cfg(test)]
mod tests;
