//! Route tree structure, dispatch, and matching algorithms.
//!
//! Defines [`RouteTree`] and [`RouteNode`], the runtime data structures
//! that hold a provider's declared route hierarchy. The tree walk
//! algorithm recursively matches VFS path segments against [`SegmentMatcher`]
//! patterns, accumulating captures in [`RouteParams`], and dispatches to
//! handler closures for `children` (readdir) and `lookup` (single-name)
//! operations.
//!
//! Rest-capture matching uses a rightmost-suffix algorithm [DD-19] that
//! tries split points from right to left, ensuring greedy consumption
//! of path segments while still allowing subsequent route levels to match.

use std::iter;
use std::ops::ControlFlow;

use super::ctx::RouteCtx;
use super::params::RouteParams;
use super::segment::{CaptureResult, SegmentMatcher};
use crate::prelude::*;

/// A hierarchical route tree for a single provider.
///
/// The tree matches VFS paths against declared patterns and dispatches
/// to handler methods. Domain-agnostic — uses only generic pattern matching
/// [DD-7, DD-15].
pub struct RouteTree<P> {
    root: RouteNode<P>,
}

/// A single node in the route tree.
///
/// Represents one level of path matching with optional handlers for
/// `children` (readdir) and `lookup` (single-name) operations. The tree
/// walk recurses through `sub_routes` matching path segments until it
/// reaches a leaf or a node with the appropriate handler.
///
/// Sub-routes are sorted by [`SegmentMatcher::precedence`] at build time
/// so the walk tries the most specific matcher first at each level.
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
/// or the full `Nodes` (`Result<Option<Vec<VirtualNode>>>`) -- the route tree
/// normalizes all forms. This ergonomic layer means providers can use the
/// simplest return type that fits their handler logic without manual wrapping.
pub trait IntoNodes {
    /// Convert into the canonical `Nodes` type for route tree dispatch.
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
///
/// Mirrors [`IntoNodes`] but for single-item lookups. Allows handlers to
/// return `Option<VirtualNode>` or the full `Node` (`Result<Option<VirtualNode>>`)
/// without manual wrapping.
pub trait IntoNode {
    /// Convert into the canonical `Node` type for route tree dispatch.
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
    ///
    /// Walks the tree matching path segments from the request context, then
    /// invokes the appropriate lookup handler at the target node. Static files
    /// are checked before dynamic handlers.
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

    /// Dispatch a lookup from explicit segments with pre-populated params.
    ///
    /// Like [`lookup`](Self::lookup), but accepts pre-split segments and existing
    /// captures. Used by companion-based providers that parse the source file
    /// path before dispatching the remaining sub-path through the route tree.
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
///
/// If the match produced a single-segment capture, inserts it into the
/// params. For `CaptureResult::None` (exact or glob matches), this is
/// a no-op that returns params unchanged.
fn apply_capture(mut params: RouteParams, capture: CaptureResult) -> RouteParams {
    if let CaptureResult::Single(name, value) = capture {
        params.insert_single(name, value);
    }
    params
}

/// Dispatch mode for the route-tree walk.
///
/// Determines whether a tree walk produces directory listings (children/readdir)
/// or single-name resolution (lookup).
enum DispatchMode<'a> {
    /// Children (readdir) — list all entries at the matched path.
    Children,
    /// Lookup — resolve a single `name` at the matched path.
    Lookup { name: &'a str },
}

/// Result from a single route match, parameterized by dispatch mode.
///
/// Wraps the mode-specific return type so `try_match` can be generic
/// over both children and lookup dispatch.
enum MatchResult {
    Children(Option<Vec<VirtualNode>>),
    Lookup(Option<VirtualNode>),
}

/// Unwrap helpers for mode-specific match results.
///
/// These are type-safe extractors that panic via `unreachable!` if called
/// on the wrong variant — this is safe because the caller always knows
/// which dispatch mode was used to produce the result.
impl MatchResult {
    /// Extract the children result, panicking if this is a lookup result.
    fn into_children(self) -> Option<Vec<VirtualNode>> {
        match self {
            Self::Children(v) => v,
            Self::Lookup(_) => unreachable!("called into_children on Lookup result"),
        }
    }

    /// Extract the lookup result, panicking if this is a children result.
    fn into_lookup(self) -> Option<VirtualNode> {
        match self {
            Self::Lookup(v) => v,
            Self::Children(_) => unreachable!("called into_lookup on Children result"),
        }
    }
}

/// Context for the rest-capture algorithm, bundling parameters that would
/// otherwise require 6+ arguments on `try_rest_capture_core`.
struct RestCaptureCtx<'a, P> {
    node: &'a RouteNode<P>,
    capture_name: &'static str,
    suffix: Option<&'a str>,
    first_segment: &'a str,
    rest: &'a [&'a str],
    params: &'a RouteParams,
}

/// Try matching a single child route for either a children or lookup dispatch.
///
/// Called once per sub-route in precedence order. Uses `ControlFlow` to
/// short-circuit: `Break(result)` means this child claimed the path and
/// dispatch is done; `Continue(())` means try the next sibling.
///
/// Handles all matcher variants: exact/capture delegate recursively,
/// rest-capture uses the rightmost-suffix algorithm, and glob invokes
/// the handler directly (it matches any remaining depth).
fn try_match<P>(
    child: &RouteNode<P>,
    provider: &P,
    ctx: &RequestContext<'_>,
    remaining: &[&str],
    params: &RouteParams,
    mode: &DispatchMode<'_>,
) -> Result<ControlFlow<MatchResult>> {
    let Some((&segment, rest)) = remaining.split_first() else {
        return Ok(ControlFlow::Continue(()));
    };
    match &child.segment {
        SegmentMatcher::RestCapture { name, suffix } => {
            let rc = RestCaptureCtx {
                node: child,
                capture_name: name,
                suffix: suffix.as_deref(),
                first_segment: segment,
                rest,
                params,
            };
            if let Some(mr) = try_rest_capture(&rc, provider, ctx, mode)? {
                return Ok(ControlFlow::Break(mr));
            }
        }
        SegmentMatcher::Glob => match mode {
            DispatchMode::Children => {
                return Ok(ControlFlow::Break(MatchResult::Children(child.invoke_children(
                    provider,
                    ctx,
                    params.clone(),
                )?)));
            }
            DispatchMode::Lookup { name } => {
                let Some(handler) = &child.lookup_handler else {
                    return Ok(ControlFlow::Continue(()));
                };
                return Ok(ControlFlow::Break(MatchResult::Lookup((handler)(
                    provider,
                    &RouteCtx::new(ctx, params.clone()),
                    name,
                )?)));
            }
        },
        other => {
            let Some(capture) = other.matches(segment) else {
                return Ok(ControlFlow::Continue(()));
            };
            let next_params = apply_capture(params.clone(), capture);
            let result = match mode {
                DispatchMode::Children =>
                    MatchResult::Children(child.dispatch_children(provider, ctx, rest, &next_params)?),
                DispatchMode::Lookup { name } =>
                    MatchResult::Lookup(child.dispatch_lookup(provider, ctx, rest, name, &next_params)?),
            };
            return Ok(ControlFlow::Break(result));
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

        let mode = DispatchMode::Children;
        // Sub-routes are sorted by precedence [DD-21]: exact > capture > rest > glob.
        for child in &self.sub_routes {
            if let ControlFlow::Break(result) = try_match(child, provider, ctx, remaining, params, &mode)? {
                return Ok(result.into_children());
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

        let mode = DispatchMode::Lookup { name };
        for child in &self.sub_routes {
            if let ControlFlow::Break(result) = try_match(child, provider, ctx, remaining, params, &mode)? {
                return Ok(result.into_lookup());
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

        let params = if let Some(handler) = &self.lookup_handler {
            let route_ctx = RouteCtx::new(ctx, params);
            let result = (handler)(provider, &route_ctx, name)?;
            if result.is_some() {
                return Ok(result);
            }
            // Reclaim params for glob fallback — avoids a redundant clone
            route_ctx.into_params()
        } else {
            params
        };

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
/// ordered from rightmost to leftmost [DD-19]. The rightmost-first ordering
/// is critical: it ensures the rest-capture consumes the maximum number of
/// segments, which matches user intent for paths like `a@/b@/c@/symbols/`
/// where `@` is the suffix and the capture should grab `a@/b@/c@`.
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
///
/// For a rest-capture like `{..path}@` matching `["a@", "b", "c@"]` with
/// split at index 2, this produces `["a", "b", "c"]` -- the `@` suffix is
/// stripped from every segment that has it, giving handlers clean values.
fn build_captured(full: &[&str], split_pos: usize, suffix: &str) -> Vec<String> {
    full.iter()
        .take(split_pos + 1)
        .map(|s| s.strip_suffix(suffix).unwrap_or(s).to_owned())
        .collect()
}

/// Core rest-capture algorithm: find split points, try each, delegate via callback.
///
/// Uses [`RestCaptureCtx`] to bundle capture parameters. The `dispatch` closure
/// receives `(remaining_segments, params_with_capture)` and performs
/// the type-specific dispatch.
fn try_rest_capture_core<P, R>(
    rc: &RestCaptureCtx<'_, P>,
    dispatch: impl Fn(&[&str], &RouteParams) -> Result<Option<R>>,
) -> Result<Option<R>> {
    let full: Vec<&str> = iter::once(rc.first_segment).chain(rc.rest.iter().copied()).collect();

    let Some(sfx) = rc.suffix else {
        // No suffix — rest capture consumes all remaining segments
        let mut params = rc.params.clone();
        params.insert_rest(rc.capture_name, full.iter().map(|s| (*s).to_owned()).collect());
        return dispatch(&[], &params);
    };

    for split_pos in find_rest_splits(&full, sfx) {
        let captured = build_captured(&full, split_pos, sfx);
        let remaining: Vec<&str> = full.get(split_pos + 1..).unwrap_or_default().to_vec();

        if remaining.is_empty() {
            let mut next_params = rc.params.clone();
            next_params.insert_rest(rc.capture_name, captured);
            return dispatch(&[], &next_params);
        }

        // Check if any child route can match the first remaining segment
        let Some(&first_remaining) = remaining.first() else {
            continue;
        };
        if rc
            .node
            .sub_routes
            .iter()
            .any(|child| child.segment.matches(first_remaining).is_some())
        {
            let mut next_params = rc.params.clone();
            next_params.insert_rest(rc.capture_name, captured);
            let result = dispatch(&remaining, &next_params)?;
            if result.is_some() {
                return Ok(result);
            }
        }
    }

    Ok(None)
}

/// Try rest-capture dispatch using the rightmost-suffix algorithm [DD-19].
///
/// Unified handler for both children and lookup modes. Delegates to
/// [`try_rest_capture_core`] with a mode-appropriate dispatch closure.
fn try_rest_capture<P>(
    rc: &RestCaptureCtx<'_, P>,
    provider: &P,
    ctx: &RequestContext<'_>,
    mode: &DispatchMode<'_>,
) -> Result<Option<MatchResult>> {
    match mode {
        DispatchMode::Children => Ok(try_rest_capture_core(rc, |remaining, params| {
            rc.node.dispatch_children(provider, ctx, remaining, params)
        })?
        .map(|v| MatchResult::Children(Some(v)))),
        DispatchMode::Lookup { name } => Ok(try_rest_capture_core(rc, |remaining, params| {
            rc.node.dispatch_lookup(provider, ctx, remaining, name, params)
        })?
        .map(|v| MatchResult::Lookup(Some(v)))),
    }
}
