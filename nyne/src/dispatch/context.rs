use super::content_cache::FileGenerations;
use super::invalidation::EventSink;
use super::resolver::Resolver;
use crate::types::TypeMap;
use crate::types::real_fs::RealFs;
use crate::types::vfs_path::VfsPath;

/// Context passed to provider and node methods during FUSE operations.
///
/// Cannot be stored across calls — all references are borrowed from
/// the router for the duration of a single FUSE operation.
pub struct RequestContext<'a> {
    pub path: &'a VfsPath,
    pub real_fs: &'a dyn RealFs,
    pub events: &'a dyn EventSink,
    pub resolver: &'a dyn Resolver,
    pub(crate) file_generations: &'a FileGenerations,
}

impl RequestContext<'_> {
    /// Current generation for a source file — used by providers to stamp
    /// companion nodes via [`VirtualNode::with_source`].
    pub fn source_generation(&self, path: &VfsPath) -> u64 { self.file_generations.get(path) }
}

/// Extended context for rename operations.
pub struct RenameContext<'a> {
    pub request: &'a RequestContext<'a>,
    pub target_path: &'a VfsPath,
    pub target_name: &'a str,
}

/// Context threaded through middleware pipelines.
///
/// Wraps a [`RequestContext`] and adds a `TypeId`-keyed extension map
/// (like `http::Extensions`) for middleware-to-middleware communication.
/// The type itself is the key — no string constants, no collision risk.
///
/// # Example
///
/// ```ignore
/// // Middleware A inserts:
/// ctx.insert(SyntaxValidated { ast_hash: 42 });
///
/// // Middleware B reads:
/// if let Some(validated) = ctx.get::<SyntaxValidated>() { ... }
/// ```
pub struct PipelineContext<'a> {
    pub request: &'a RequestContext<'a>,
    extensions: TypeMap,
}

impl<'a> PipelineContext<'a> {
    /// Create a pipeline context from a request context.
    pub fn new(request: &'a RequestContext<'a>) -> Self {
        Self {
            request,
            extensions: TypeMap::new(),
        }
    }

    /// Insert a typed extension. Replaces any existing value of the same type.
    pub fn insert<T: Send + Sync + 'static>(&mut self, value: T) { self.extensions.insert(value); }

    /// Retrieve a typed extension by type.
    pub fn get<T: 'static>(&self) -> Option<&T> { self.extensions.get::<T>() }
}
