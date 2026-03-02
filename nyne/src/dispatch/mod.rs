//! Route and cache FUSE operations through provider and mutation layers.
//!
//! Invariant: This layer MUST NOT import from `fuser` or `crate::fuse`.
//! The FUSE layer depends on dispatch, never the reverse.

// Interface types — pub for plugin crates
pub mod activation;
pub mod context;
pub mod invalidation;
pub mod resolver;
pub mod routing;
pub mod script;
pub mod write_mode;

// Implementation — stays crate-internal
pub(crate) mod cache;
pub(crate) mod content_cache;
pub(crate) mod events;
pub(crate) mod inode;
pub(crate) mod mutation;
pub(crate) mod path_filter;
pub(crate) mod pipeline;
pub(crate) mod registry;
pub(crate) mod resolve;
pub(crate) mod router;
pub(crate) mod script_registry;

pub use events::{BufferedEventSink, LoggingEventSink};
pub use registry::ProviderRegistry;
pub use router::{ReaddirEntry, ResolvedInode, Router};
pub use script_registry::ScriptRegistry;
