use nyne::router::Request;

/// Visibility mode set by [`VisibilityProvider`](super::VisibilityProvider).
///
/// Downstream providers check [`req.visibility()`](VisibilityRequest::visibility)
/// to decide whether to emit nodes for the current request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// Default: resolve on lookup, suppress on readdir.
    /// The path exists but isn't listed.
    Default,
    /// Force visibility: always emit on both readdir and lookup.
    Force,
    /// Hidden: suppress on both readdir and lookup.
    Hidden,
}

/// Extension trait for reading visibility state from a [`Request`].
pub trait VisibilityRequest {
    /// The visibility mode for this request, if set by [`VisibilityProvider`](super::VisibilityProvider).
    fn visibility(&self) -> Option<Visibility>;
}

impl VisibilityRequest for Request {
    fn visibility(&self) -> Option<Visibility> { self.state::<Visibility>().copied() }
}

/// Policy function that determines visibility for a given request.
///
/// Receives the current request and returns `Some(Visibility)` to set
/// visibility state, or `None` to leave it unset (no filtering).
pub type VisibilityPolicy = Box<dyn Fn(&Request) -> Option<Visibility> + Send + Sync>;
