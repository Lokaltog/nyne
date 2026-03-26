//! Route and cache FUSE operations through provider and mutation layers.
//!
//! Invariant: This layer MUST NOT import from `fuser` or `crate::fuse`.
//! The FUSE layer depends on dispatch, never the reverse.

// Interface types — pub for plugin crates
/// Shared context provided to providers during plugin activation.
pub mod activation;
/// Request and pipeline context types threaded through FUSE operations.
pub mod context;
/// Cache invalidation traits, events, and kernel notification.
pub mod invalidation;
/// Virtual path-to-node resolution with recursion guard.
pub mod resolver;
/// Hierarchical route tree matching and dispatch for provider namespaces.
pub mod routing;
/// Script execution context, traits, and addressing.
pub mod script;

/// How the FUSE write should be dispatched to the [`Writable`](crate::node::Writable) capability.
///
/// Derived from the open flags (`O_TRUNC`, `O_APPEND`) stored on the file handle
/// at open time. The [`Pipeline`] threads this through the middleware chain so the
/// final `Writable` method is chosen correctly without re-inspecting flags.
///
/// Marked `#[non_exhaustive]` because additional modes (e.g., `Append`) may be
/// added as providers gain richer write semantics.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    /// Standard positional write (default).
    Normal,
    /// File was opened with `O_TRUNC` — full content replacement.
    Truncate,
}

// Implementation — stays crate-internal
/// L1 directory structure cache with per-directory resolve generations.
pub(crate) mod cache;
/// L2 content cache for generated file data with generation-based staleness.
pub(crate) mod content_cache;
/// Event sink implementations for invalidation event collection.
pub(crate) mod events;
/// Bidirectional inode number to VFS location mapping.
pub(crate) mod inode;
/// File system mutation operations: create, remove, rename with provider delegation.
pub(crate) mod mutation;
/// Fast path filter for skipping provider resolution on gitignored paths.
pub(crate) mod path_filter;
/// Middleware pipeline for read/write request processing.
pub(crate) mod pipeline;
/// Registry of activated providers and their construction.
pub(crate) mod registry;
/// Directory resolution pipeline with multi-provider conflict negotiation.
pub(crate) mod resolve;
/// Core FUSE operation router with caching and inode map.
pub(crate) mod router;
/// Registry of named scripts indexed by dotted address.
pub(crate) mod script_registry;

pub use events::{BufferedEventSink, LoggingEventSink};
pub use registry::ProviderRegistry;
pub use router::{ReaddirEntry, ResolvedInode, Router};
pub use script_registry::ScriptRegistry;
