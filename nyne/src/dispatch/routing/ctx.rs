use std::ops::Deref;

use super::params::RouteParams;
use crate::dispatch::context::RequestContext;

/// Handler context — wraps `RequestContext` with route captures.
///
/// Provides `param()` / `params()` for captures, and derefs to
/// `RequestContext` for path, `real_fs`, events, resolver access.
pub struct RouteCtx<'a> {
    request: &'a RequestContext<'a>,
    params: RouteParams,
}

impl<'a> RouteCtx<'a> {
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
}

impl<'a> Deref for RouteCtx<'a> {
    type Target = RequestContext<'a>;

    fn deref(&self) -> &Self::Target { self.request }
}
