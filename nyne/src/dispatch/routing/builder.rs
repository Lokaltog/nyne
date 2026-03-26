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
use crate::prelude::*;

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
    ///
    /// This is the primary way to attach segment-matched sub-trees. Each child
    /// is a [`RouteNodeBuilder`] configured with a segment matcher and handlers.
    #[must_use]
    pub fn route(mut self, child: RouteNodeBuilder<P>) -> Self {
        self.root.sub_routes.push(child);
        self
    }

    /// Set the readdir (children) handler at the root level.
    ///
    /// Invoked when the VFS path resolves to the root of this route tree.
    /// Returns directory entries visible at the top level of the provider's namespace.
    #[must_use]
    pub fn children(mut self, handler: impl Fn(&P, &RouteCtx<'_>) -> Nodes + Send + Sync + 'static) -> Self {
        self.root.children_handler = Some(Box::new(handler));
        self
    }

    /// Set the single-name lookup handler at the root level.
    ///
    /// Invoked when the VFS looks up a specific name within the root of this
    /// route tree. Returns the matching node or `None` if unrecognized.
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

    /// Create a node that matches a single exact path segment (e.g., `"symbols"`).
    ///
    /// Exact nodes auto-emit a directory entry in the parent's readdir by default.
    /// Call [`no_emit`](Self::no_emit) to suppress this for lookup-only routes.
    pub fn exact(name: &'static str) -> Self { Self::new(SegmentMatcher::Exact(name)) }

    /// Create a node that captures a single path segment into a named parameter.
    ///
    /// The captured value is accessible via [`RouteCtx::param`]. Optional `prefix`
    /// and `suffix` are stripped before storing (e.g., `prefix: Some("@")` matches
    /// `"@foo"` and captures `"foo"`).
    pub fn capture(name: &'static str, prefix: Option<&'static str>, suffix: Option<&'static str>) -> Self {
        Self::new(SegmentMatcher::Capture { name, prefix, suffix })
    }

    /// Create a node that greedily captures one or more remaining path segments.
    ///
    /// The captured segments are accessible via [`RouteCtx::params`] as a `Vec`.
    /// Uses the rightmost-suffix algorithm [DD-19] to find split points when
    /// subsequent route levels need to match after the rest-capture.
    pub fn rest_capture(name: &'static str, suffix: Option<&'static str>) -> Self {
        Self::new(SegmentMatcher::RestCapture { name, suffix })
    }

    /// Create a catch-all glob node that matches any single segment.
    ///
    /// Glob nodes have the lowest precedence and act as fallback routes
    /// when no exact, capture, or rest-capture matches. They do not emit
    /// directory entries and do not capture the matched segment name.
    pub fn glob() -> Self { Self::new(SegmentMatcher::Glob) }

    /// Set the handler invoked for readdir (children) requests at this node.
    ///
    /// The handler receives the provider state and route context, and returns
    /// the list of virtual nodes to expose as directory entries. Static files
    /// and auto-emitted sub-route directories are merged into the result.
    #[must_use]
    pub fn children(mut self, handler: impl Fn(&P, &RouteCtx<'_>) -> Nodes + Send + Sync + 'static) -> Self {
        self.children_handler = Some(Box::new(handler));
        self
    }

    /// Set the handler invoked for single-name lookup requests at this node.
    ///
    /// The handler receives the provider state, route context, and the name
    /// being looked up. It returns `Ok(Some(node))` for a match or `Ok(None)`
    /// to indicate no match. Static files are checked before invoking this handler.
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

    /// Add a child sub-route nested under this node.
    ///
    /// Sub-routes are sorted by segment precedence at build time (exact before
    /// capture before rest-capture before glob), ensuring deterministic dispatch.
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
