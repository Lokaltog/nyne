//! Fluent builder API for constructing route trees.
//!
//! Provides [`RouteTreeBuilder`] and [`RouteNodeBuilder`] for assembling
//! route trees programmatically. This is the target API for the `routes!`
//! proc-macro codegen, but can also be used directly when the macro's
//! syntax is insufficient (escape hatch, see DD-11).
//!
//! The builder enforces correct structure at compile time via the type
//! parameter `P` (the provider state type), ensuring handler closures
//! receive the right provider reference.

use super::ctx::RouteCtx;
use super::segment::SegmentMatcher;
use super::tree::{ChildrenHandler, LookupHandler, RouteNode, RouteTree, StaticFileEntry};
use crate::node::VirtualNode;
use crate::provider::{Node, Nodes};

/// Builder for constructing route trees programmatically.
///
/// This is the underlying API that the `routes!` proc-macro generates
/// calls to. Can also be used directly as an escape hatch [DD-11].
pub struct RouteTreeBuilder<P> {
    root: RouteNodeBuilder<P>,
}

/// Builder for constructing a single route node with handlers and sub-routes.
///
/// Each node represents one level in the route tree hierarchy and binds a
/// [`SegmentMatcher`] to optional `children`/`lookup` handlers, static
/// files, and nested sub-routes. Convenience constructors (`exact`,
/// `capture`, `rest_capture`, `glob`) create a node with the appropriate
/// matcher pre-configured.
///
/// By default, `Exact` nodes auto-emit a directory entry in the parent's
/// readdir. Call [`no_emit`](Self::no_emit) to suppress this for
/// lookup-only routes (e.g., companion `@` directories).
pub struct RouteNodeBuilder<P> {
    segment: SegmentMatcher,
    children_handler: Option<ChildrenHandler<P>>,
    lookup_handler: Option<LookupHandler<P>>,
    static_files: Vec<StaticFileEntry>,
    sub_routes: Vec<Self>,
    emit: bool,
}

/// Default implementation for `RouteTreeBuilder`.
impl<P: Send + Sync + 'static> Default for RouteTreeBuilder<P> {
    /// Delegates to [`RouteTreeBuilder::new`].
    fn default() -> Self { Self::new() }
}

/// Root-level builder methods for constructing a route tree.
impl<P: Send + Sync + 'static> RouteTreeBuilder<P> {
    /// Create an empty route tree builder with a root node.
    pub fn new() -> Self {
        Self {
            root: RouteNodeBuilder::new(SegmentMatcher::Root),
        }
    }

    /// Add a child route at the root level.
    #[must_use]
    pub fn route(mut self, child: RouteNodeBuilder<P>) -> Self {
        self.root.sub_routes.push(child);
        self
    }

    /// Set children handler at root level.
    #[must_use]
    pub fn children(mut self, handler: impl Fn(&P, &RouteCtx<'_>) -> Nodes + Send + Sync + 'static) -> Self {
        self.root.children_handler = Some(Box::new(handler));
        self
    }

    /// Set lookup handler at root level.
    #[must_use]
    pub fn lookup(mut self, handler: impl Fn(&P, &RouteCtx<'_>, &str) -> Node + Send + Sync + 'static) -> Self {
        self.root.lookup_handler = Some(Box::new(handler));
        self
    }

    /// Build the route tree. Sorts sub-routes by precedence.
    pub fn build(self) -> RouteTree<P> { RouteTree::from_root(self.root.build()) }
}

/// Node-level builder methods: segment matching, handlers, static files, sub-routes.
impl<P: Send + Sync + 'static> RouteNodeBuilder<P> {
    /// Create a route node builder for the given segment matcher.
    pub fn new(segment: SegmentMatcher) -> Self {
        let emit = matches!(segment, SegmentMatcher::Exact(_));
        Self {
            segment,
            children_handler: None,
            lookup_handler: None,
            static_files: Vec::new(),
            sub_routes: Vec::new(),
            emit,
        }
    }

    /// Suppress auto-emission of a directory entry in the parent's readdir.
    /// By default, Exact sub-routes emit; call this for lookup-only routes.
    #[must_use]
    pub const fn no_emit(mut self) -> Self {
        self.emit = false;
        self
    }

    /// Exact segment match.
    pub fn exact(name: &'static str) -> Self { Self::new(SegmentMatcher::Exact(name)) }

    /// Single-segment capture with optional prefix and/or suffix.
    pub fn capture(name: &'static str, prefix: Option<&'static str>, suffix: Option<&'static str>) -> Self {
        Self::new(SegmentMatcher::Capture { name, prefix, suffix })
    }

    /// Rest capture (1+ segments).
    pub fn rest_capture(name: &'static str, suffix: Option<&'static str>) -> Self {
        Self::new(SegmentMatcher::RestCapture { name, suffix })
    }

    /// Glob match.
    pub fn glob() -> Self { Self::new(SegmentMatcher::Glob) }

    /// Set children handler.
    #[must_use]
    pub fn children(mut self, handler: impl Fn(&P, &RouteCtx<'_>) -> Nodes + Send + Sync + 'static) -> Self {
        self.children_handler = Some(Box::new(handler));
        self
    }

    /// Set lookup handler.
    #[must_use]
    pub fn lookup(mut self, handler: impl Fn(&P, &RouteCtx<'_>, &str) -> Node + Send + Sync + 'static) -> Self {
        self.lookup_handler = Some(Box::new(handler));
        self
    }

    /// Add a static file via factory closure.
    ///
    /// Static files appear in both `children` (readdir) and `lookup`
    /// responses for this node. The factory is called on each access
    /// because `VirtualNode` is not `Clone` (it may hold `Box<dyn Readable>`).
    /// Use this for lightweight, always-present entries like `OVERVIEW.md`.
    #[must_use]
    pub fn file(mut self, name: &'static str, factory: impl Fn() -> VirtualNode + Send + Sync + 'static) -> Self {
        self.static_files.push(StaticFileEntry {
            name,
            factory: Box::new(factory),
        });
        self
    }

    /// Add a sub-route.
    #[must_use]
    pub fn route(mut self, child: Self) -> Self {
        self.sub_routes.push(child);
        self
    }

    /// Build into a `RouteNode`, sorting children by precedence.
    ///
    /// Sub-routes are sorted so that more specific matchers (exact) are
    /// tried before less specific ones (capture, rest-capture, glob).
    /// This ensures deterministic dispatch order per [DD-21]. The sort
    /// is recursive -- each child's sub-routes are also sorted.
    pub(super) fn build(mut self) -> RouteNode<P> {
        self.sub_routes.sort_by_key(|r| r.segment.precedence());
        RouteNode {
            segment: self.segment,
            children_handler: self.children_handler,
            lookup_handler: self.lookup_handler,
            static_files: self.static_files,
            sub_routes: self.sub_routes.into_iter().map(Self::build).collect(),
            emit: self.emit,
        }
    }
}
