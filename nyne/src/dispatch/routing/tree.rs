use std::iter;
use std::ops::ControlFlow;

use color_eyre::eyre::Result;

use super::ctx::RouteCtx;
use super::params::RouteParams;
use super::segment::{CaptureResult, SegmentMatcher};
use crate::dispatch::context::RequestContext;
use crate::node::VirtualNode;
use crate::provider::{Node, Nodes};

/// A hierarchical route tree for a single provider.
///
/// The tree matches VFS paths against declared patterns and dispatches
/// to handler methods. Domain-agnostic — uses only generic pattern matching
/// [DD-7, DD-15].
pub struct RouteTree<P> {
    root: RouteNode<P>,
}

/// A single node in the route tree.
pub(super) struct RouteNode<P> {
    pub segment: SegmentMatcher,
    pub children_handler: Option<ChildrenHandler<P>>,
    pub lookup_handler: Option<LookupHandler<P>>,
    pub static_files: Vec<StaticFileEntry>,
    pub sub_routes: Vec<Self>,
    /// When false, suppresses the default auto-emission of a directory entry
    /// in the parent's readdir. Exact sub-routes emit by default; use
    /// `.no_emit()` on the builder to suppress for lookup-only routes.
    pub emit: bool,
}

/// Inline file declaration — build-time Readable [DD-18].
///
/// Stores a factory closure + name because `VirtualNode` isn't Clone
/// (it holds `Box<dyn Readable>` etc.). Fresh nodes are produced on each readdir.
pub(super) struct StaticFileEntry {
    pub name: &'static str,
    pub factory: Box<dyn Fn() -> VirtualNode + Send + Sync>,
}

/// Handler for readdir (children) requests.
pub(super) type ChildrenHandler<P> = Box<dyn Fn(&P, &RouteCtx<'_>) -> Nodes + Send + Sync>;

/// Handler for lookup (single name) requests.
pub(super) type LookupHandler<P> = Box<dyn Fn(&P, &RouteCtx<'_>, &str) -> Node + Send + Sync>;

/// Conversion trait for children handler return values.
///
/// Allows handler methods to return `Vec<VirtualNode>`, `Option<Vec<VirtualNode>>`,
/// or the full `Nodes` — the route tree normalizes all forms.
pub trait IntoNodes {
    fn into_nodes(self) -> Nodes;
}

/// Identity conversion for `Nodes`.
impl IntoNodes for Nodes {
    /// Pass through unchanged.
    fn into_nodes(self) -> Nodes { self }
}

/// Wraps a `Vec<VirtualNode>` into `Ok(Some(...))`.
impl IntoNodes for Vec<VirtualNode> {
    /// Wrap into `Ok(Some(self))`.
    fn into_nodes(self) -> Nodes { Ok(Some(self)) }
}

/// Wraps an `Option<Vec<VirtualNode>>` into `Ok(...)`.
impl IntoNodes for Option<Vec<VirtualNode>> {
    /// Wrap into `Ok(self)`.
    fn into_nodes(self) -> Nodes { Ok(self) }
}

/// Conversion trait for lookup handler return values.
pub trait IntoNode {
    fn into_node(self) -> Node;
}

/// Identity conversion for `Node`.
impl IntoNode for Node {
    /// Pass through unchanged.
    fn into_node(self) -> Node { self }
}

/// Wraps an `Option<VirtualNode>` into `Ok(...)`.
impl IntoNode for Option<VirtualNode> {
    /// Wrap into `Ok(self)`.
    fn into_node(self) -> Node { Ok(self) }
}

/// Public dispatch API for route trees: children, lookup, and rebuild.
impl<P> RouteTree<P> {
    /// Create a route tree from a root node.
    pub(super) const fn from_root(root: RouteNode<P>) -> Self { Self { root } }

    /// Dispatch a children (readdir) request.
    ///
    /// Walks the tree matching path segments, accumulating captures.
    /// Returns merged handler output + static file nodes.
    pub fn children(&self, provider: &P, ctx: &RequestContext<'_>) -> Nodes {
        let segments = ctx.path.segments();
        self.root
            .dispatch_children(provider, ctx, &segments, &RouteParams::default())
    }

    /// Dispatch a lookup request for a specific name.
    pub fn lookup(&self, provider: &P, ctx: &RequestContext<'_>, name: &str) -> Node {
        let segments = ctx.path.segments();
        self.root
            .dispatch_lookup(provider, ctx, &segments, name, &RouteParams::default())
    }

    /// Dispatch children from explicit segments with pre-populated params.
    ///
    /// Used by companion-based providers that extract the source file path
    /// before dispatching the remaining sub-path through the route tree.
    pub fn children_at(
        &self,
        provider: &P,
        ctx: &RequestContext<'_>,
        segments: &[&str],
        params: &RouteParams,
    ) -> Nodes {
        self.root.dispatch_children(provider, ctx, segments, params)
    }

    /// Dispatch lookup from explicit segments with pre-populated params.
    pub fn lookup_at(
        &self,
        provider: &P,
        ctx: &RequestContext<'_>,
        segments: &[&str],
        name: &str,
        params: &RouteParams,
    ) -> Node {
        self.root.dispatch_lookup(provider, ctx, segments, name, params)
    }

    /// Re-invoke handlers to rebuild a specific node (for `on_conflict`).
    pub fn rebuild_node(&self, provider: &P, ctx: &RequestContext<'_>, name: &str) -> Node {
        let nodes = self.children(provider, ctx)?;
        Ok(nodes.and_then(|ns| ns.into_iter().find(|n| n.name() == name)))
    }
}

/// Fold a capture result into route params.
fn apply_capture(mut params: RouteParams, capture: CaptureResult) -> RouteParams {
    if let CaptureResult::Single(name, value) = capture {
        params.insert_single(name, value);
    }
    params
}

/// Try matching a single child route for a children dispatch.
///
/// Returns `Break(nodes)` if the child matched, `Continue(())` to try the next child.
fn try_match_children<P>(
    child: &RouteNode<P>,
    provider: &P,
    ctx: &RequestContext<'_>,
    remaining: &[&str],
    params: &RouteParams,
) -> Result<ControlFlow<Option<Vec<VirtualNode>>>> {
    let Some((&segment, rest)) = remaining.split_first() else {
        return Ok(ControlFlow::Continue(()));
    };
    match &child.segment {
        SegmentMatcher::RestCapture { name, suffix } => {
            let result = try_rest_capture(child, provider, ctx, name, suffix.as_deref(), segment, rest, params)?;
            if result.is_some() {
                return Ok(ControlFlow::Break(result));
            }
        }
        SegmentMatcher::Glob => {
            return Ok(ControlFlow::Break(child.invoke_children(
                provider,
                ctx,
                params.clone(),
            )?));
        }
        other => {
            let Some(capture) = other.matches(segment) else {
                return Ok(ControlFlow::Continue(()));
            };
            return Ok(ControlFlow::Break(child.dispatch_children(
                provider,
                ctx,
                rest,
                &apply_capture(params.clone(), capture),
            )?));
        }
    }
    Ok(ControlFlow::Continue(()))
}

/// Try matching a single child route for a lookup dispatch.
fn try_match_lookup<P>(
    child: &RouteNode<P>,
    provider: &P,
    ctx: &RequestContext<'_>,
    remaining: &[&str],
    lookup_name: &str,
    params: &RouteParams,
) -> Result<ControlFlow<Option<VirtualNode>>> {
    let Some((&segment, rest)) = remaining.split_first() else {
        return Ok(ControlFlow::Continue(()));
    };
    match &child.segment {
        SegmentMatcher::RestCapture { name, suffix } => {
            let result = try_rest_capture_lookup(
                child,
                provider,
                ctx,
                name,
                suffix.as_deref(),
                segment,
                rest,
                lookup_name,
                params,
            )?;
            if result.is_some() {
                return Ok(ControlFlow::Break(result));
            }
        }
        SegmentMatcher::Glob => {
            let Some(handler) = &child.lookup_handler else {
                return Ok(ControlFlow::Continue(()));
            };
            return Ok(ControlFlow::Break((handler)(
                provider,
                &RouteCtx::new(ctx, params.clone()),
                lookup_name,
            )?));
        }
        other => {
            let Some(capture) = other.matches(segment) else {
                return Ok(ControlFlow::Continue(()));
            };
            return Ok(ControlFlow::Break(child.dispatch_lookup(
                provider,
                ctx,
                rest,
                lookup_name,
                &apply_capture(params.clone(), capture),
            )?));
        }
    }
    Ok(ControlFlow::Continue(()))
}

/// Internal dispatch and handler invocation for route nodes.
impl<P> RouteNode<P> {
    /// Dispatch children request — recursive tree walk.
    fn dispatch_children(
        &self,
        provider: &P,
        ctx: &RequestContext<'_>,
        remaining: &[&str],
        params: &RouteParams,
    ) -> Nodes {
        if remaining.is_empty() {
            return self.invoke_children(provider, ctx, params.clone());
        }

        // Sub-routes are sorted by precedence [DD-21]: exact > capture > rest > glob.
        for child in &self.sub_routes {
            if let ControlFlow::Break(result) = try_match_children(child, provider, ctx, remaining, params)? {
                return Ok(result);
            }
        }

        Ok(None)
    }

    /// Dispatch lookup request — recursive tree walk.
    fn dispatch_lookup(
        &self,
        provider: &P,
        ctx: &RequestContext<'_>,
        remaining: &[&str],
        name: &str,
        params: &RouteParams,
    ) -> Node {
        if remaining.is_empty() {
            return self.invoke_lookup(provider, ctx, params.clone(), name);
        }

        for child in &self.sub_routes {
            if let ControlFlow::Break(result) = try_match_lookup(child, provider, ctx, remaining, name, params)? {
                return Ok(result);
            }
        }

        Ok(None)
    }

    /// Invoke children handler + merge static files + auto-emit sub-route directories.
    fn invoke_children(&self, provider: &P, ctx: &RequestContext<'_>, params: RouteParams) -> Nodes {
        let route_ctx = RouteCtx::new(ctx, params);

        let mut nodes = if let Some(handler) = &self.children_handler {
            (handler)(provider, &route_ctx)?.unwrap_or_default()
        } else {
            Vec::new()
        };

        // Append static file nodes [DD-17: concatenation, no dedup]
        for entry in &self.static_files {
            nodes.push((entry.factory)());
        }

        // Emit directory entries for sub-routes that opted in (Exact default).
        // Lookup-only routes (e.g. `@` companion dirs) use `.no_emit()`.
        for child in &self.sub_routes {
            if child.emit
                && let SegmentMatcher::Exact(name) = child.segment
            {
                nodes.push(VirtualNode::directory(name));
            }
        }

        if nodes.is_empty() { Ok(None) } else { Ok(Some(nodes)) }
    }

    /// Invoke lookup handler if present, checking static files first.
    /// Falls through to Glob sub-routes so `"**" => lookup(handler)` works at every depth.
    fn invoke_lookup(&self, provider: &P, ctx: &RequestContext<'_>, params: RouteParams, name: &str) -> Node {
        // Check static files first (cheap name comparison, no factory call)
        for entry in &self.static_files {
            if entry.name == name {
                return Ok(Some((entry.factory)()));
            }
        }

        if let Some(handler) = &self.lookup_handler {
            let route_ctx = RouteCtx::new(ctx, params.clone());
            let result = (handler)(provider, &route_ctx, name)?;
            if result.is_some() {
                return Ok(result);
            }
        }

        // Glob fallback: a "**" sub-route's lookup handler applies at any depth.
        for child in &self.sub_routes {
            if matches!(child.segment, SegmentMatcher::Glob)
                && let Some(handler) = &child.lookup_handler
            {
                return (handler)(provider, &RouteCtx::new(ctx, params), name);
            }
        }

        Ok(None)
    }
}

/// Find split points where a suffix-bearing rest-capture can terminate.
///
/// Returns indices (into `full`) of segments that end with the given suffix,
/// ordered from rightmost to leftmost [DD-19].
fn find_rest_splits(full: &[&str], suffix: &str) -> Vec<usize> {
    let mut positions: Vec<usize> = full
        .iter()
        .enumerate()
        .filter(|(_, s)| s.ends_with(suffix))
        .map(|(i, _)| i)
        .collect();
    positions.reverse();
    positions
}

/// Build captured segments from a split point, stripping suffix from every
/// segment that carries it (not just the terminator).
fn build_captured(full: &[&str], split_pos: usize, suffix: &str) -> Vec<String> {
    full.iter()
        .take(split_pos + 1)
        .map(|s| s.strip_suffix(suffix).unwrap_or(s).to_owned())
        .collect()
}

/// Core rest-capture algorithm: find split points, try each, delegate via callback.
///
/// Shared by `try_rest_capture` (children) and `try_rest_capture_lookup` (lookup).
/// The `dispatch` closure receives `(remaining_segments, params_with_capture)` and
/// performs the type-specific dispatch.
#[allow(clippy::too_many_arguments)]
fn try_rest_capture_core<P, R>(
    sub_routes: &[RouteNode<P>],
    capture_name: &'static str,
    suffix: Option<&str>,
    first_segment: &str,
    rest: &[&str],
    params: &RouteParams,
    dispatch: impl Fn(&[&str], &RouteParams) -> Result<Option<R>>,
) -> Result<Option<R>> {
    let full: Vec<&str> = iter::once(first_segment).chain(rest.iter().copied()).collect();

    let Some(sfx) = suffix else {
        // No suffix — rest capture consumes all remaining segments
        let mut params = params.clone();
        params.insert_rest(capture_name, full.iter().map(|s| (*s).to_owned()).collect());
        return dispatch(&[], &params);
    };

    for split_pos in find_rest_splits(&full, sfx) {
        let captured = build_captured(&full, split_pos, sfx);
        let remaining: Vec<&str> = full.get(split_pos + 1..).unwrap_or_default().to_vec();

        if remaining.is_empty() {
            let mut next_params = params.clone();
            next_params.insert_rest(capture_name, captured);
            return dispatch(&[], &next_params);
        }

        // Check if any child route can match the first remaining segment
        let Some(&first_remaining) = remaining.first() else {
            continue;
        };
        if sub_routes
            .iter()
            .any(|child| child.segment.matches(first_remaining).is_some())
        {
            let mut next_params = params.clone();
            next_params.insert_rest(capture_name, captured);
            let result = dispatch(&remaining, &next_params)?;
            if result.is_some() {
                return Ok(result);
            }
        }
    }

    Ok(None)
}

/// Try rest-capture dispatch for children, using rightmost-suffix algorithm [DD-19].
#[allow(clippy::too_many_arguments)]
fn try_rest_capture<P>(
    node: &RouteNode<P>,
    provider: &P,
    ctx: &RequestContext<'_>,
    capture_name: &'static str,
    suffix: Option<&str>,
    first_segment: &str,
    rest: &[&str],
    params: &RouteParams,
) -> Result<Option<Vec<VirtualNode>>> {
    try_rest_capture_core(
        &node.sub_routes,
        capture_name,
        suffix,
        first_segment,
        rest,
        params,
        |remaining, params| node.dispatch_children(provider, ctx, remaining, params),
    )
}

/// Try rest-capture dispatch for lookup, using rightmost-suffix algorithm [DD-19].
#[allow(clippy::too_many_arguments)]
fn try_rest_capture_lookup<P>(
    node: &RouteNode<P>,
    provider: &P,
    ctx: &RequestContext<'_>,
    capture_name: &'static str,
    suffix: Option<&str>,
    first_segment: &str,
    rest: &[&str],
    lookup_name: &str,
    params: &RouteParams,
) -> Result<Option<VirtualNode>> {
    try_rest_capture_core(
        &node.sub_routes,
        capture_name,
        suffix,
        first_segment,
        rest,
        params,
        |remaining, params| node.dispatch_lookup(provider, ctx, remaining, lookup_name, params),
    )
}
