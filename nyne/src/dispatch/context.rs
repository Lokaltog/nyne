//! Request and pipeline context types threaded through FUSE operations.
//!
//! Every FUSE callback receives a [`RequestContext`] with borrowed references
//! to shared services (filesystem, event sink, resolver, generation tracker).
//! For middleware pipelines, [`PipelineContext`] wraps it and adds a typed
//! extension map for inter-middleware communication.
//!
//! These are **interface types** — they appear in trait signatures across all
//! tiers and may be imported freely.

use super::content_cache::FileGenerations;
use super::invalidation::EventSink;
use super::resolver::Resolver;
use crate::types::TypeMap;
use crate::types::real_fs::RealFs;
use crate::types::vfs_path::VfsPath;

/// Context passed to provider and node methods during FUSE operations.
///
/// Created fresh for every FUSE callback by the router. All references are
/// borrowed from router-owned state for the duration of that single operation,
/// so this type **cannot be stored** across calls — it exists only on the stack.
///
/// Providers receive this to access the virtual path being operated on, emit
/// invalidation events, delegate sub-resolution via the [`Resolver`], and
/// query file generation counters for cache-aware content stamping.
pub struct RequestContext<'a> {
    /// The virtual path this operation targets (e.g., `src/lib.rs@/symbols/Foo`).
    pub path: &'a VfsPath,
    /// Filesystem access for reading real (non-virtual) files on the overlay.
    pub real_fs: &'a dyn RealFs,
    /// Sink for emitting cache invalidation events during the operation.
    pub events: &'a dyn EventSink,
    /// Resolver for recursive sub-directory resolution (e.g., a provider that
    /// needs to resolve a child namespace during its own `resolve` call).
    pub resolver: &'a dyn Resolver,
    /// Per-file generation counters — `pub(crate)` because providers should use
    /// [`source_generation()`](Self::source_generation) rather than accessing
    /// the map directly.
    pub(crate) file_generations: &'a FileGenerations,
}

/// Helper methods for querying request-scoped state.
impl RequestContext<'_> {
    /// Current generation for a source file — used by providers to stamp
    /// companion nodes via [`VirtualNode::with_source`].
    pub fn source_generation(&self, path: &VfsPath) -> u64 { self.file_generations.get(path) }
}

/// Extended context for rename operations, carrying both source and destination info.
///
/// A rename involves two directories (source and target, which may be the same).
/// The base [`RequestContext`] carries the source path, while this struct adds
/// the target path and the new name. Providers implementing
/// [`Renameable`](crate::node::Renameable) receive this to decide whether to
/// accept the rename and where to relocate the node.
pub struct RenameContext<'a> {
    /// The base request context — `request.path` is the **source** directory.
    pub request: &'a RequestContext<'a>,
    /// The **target** directory path (may equal `request.path` for same-dir renames).
    pub target_path: &'a VfsPath,
    /// The new name for the entry in the target directory.
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

/// Construction and typed extension access for pipeline contexts.
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
