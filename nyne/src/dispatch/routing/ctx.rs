//! Route handler context with captured parameters.
//!
//! Defines [`RouteCtx`], the context object passed to every route handler.
//! It combines the [`RequestContext`](crate::dispatch::context::RequestContext)
//! (path, services, resolver) with [`RouteParams`] (captured segments from
//! pattern matching), and provides `Deref` to `RequestContext` for ergonomic
//! access to request-scoped services.

use std::ops::Deref;

use super::params::RouteParams;
use crate::dispatch::context::RequestContext;

/// Handler context -- wraps [`RequestContext`] with route captures.
///
/// Every route handler receives a `RouteCtx` that provides two things:
///
/// 1. **Captured parameters** via [`param`](Self::param) (single-segment)
///    and [`params`](Self::params) (rest-capture). These are accumulated
///    as the route tree walks through matching segments.
/// 2. **Request services** via `Deref<Target = RequestContext>` -- path,
///    `real_fs`, events, resolver, and file-generation tracking are all
///    accessible without explicit unwrapping.
///
/// Constructed by the route tree dispatch machinery; handlers should not
/// need to create these manually.
pub struct RouteCtx<'a> {
    request: &'a RequestContext<'a>,
    params: RouteParams,
}

/// Construction and capture accessors for route handler contexts.
impl<'a> RouteCtx<'a> {
    /// Create a route context from a request context and captured parameters.
    pub const fn new(request: &'a RequestContext<'a>, params: RouteParams) -> Self { Self { request, params } }

    /// Get a single-segment capture by name.
    ///
    /// # Panics
    /// Panics if the capture doesn't exist in the current route ancestry.
    pub fn param(&self, name: &str) -> &str { self.params.get(name) }

    /// Get a rest capture by name (1+ segments).
    ///
    /// # Panics
    /// Panics if the capture doesn't exist in the current route ancestry.
    pub fn params(&self, name: &str) -> &[String] { self.params.get_rest(name) }

    /// Access the underlying `RouteParams` (used by generated lookup closures).
    pub const fn route_params(&self) -> &RouteParams { &self.params }

    /// Access the underlying `RequestContext` (used by generated lookup closures).
    pub const fn request(&self) -> &RequestContext<'a> { self.request }

    /// Consume the context and return the owned route parameters.
    pub fn into_params(self) -> RouteParams { self.params }
}

/// Derefs to the underlying [`RequestContext`] for transparent access.
impl<'a> Deref for RouteCtx<'a> {
    /// The deref target type.
    type Target = RequestContext<'a>;

    /// Returns a reference to the inner `RequestContext`.
    fn deref(&self) -> &Self::Target { self.request }
}
