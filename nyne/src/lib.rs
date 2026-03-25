//! Nyne — expose source code as a FUSE filesystem.
extern crate self as nyne;

/// Error utilities for FUSE errno handling and eyre integration.
pub mod err;

/// Command-line interface: argument parsing and subcommand dispatch.
pub mod cli;
/// Configuration loading and validation from TOML.
pub mod config;
/// Request dispatch: routing, caching, and content pipeline.
pub mod dispatch;
/// Shared text utilities: slugification, date formatting, diffs.
pub mod text;
// Re-export provider helpers for plugin crates.
pub use providers::names::{FILE_OVERVIEW, SUBDIR_SYMBOLS};
pub use providers::{
    companion_children, companion_dir, companion_lookup, companion_symbol_path, dispatch_children, dispatch_lookup,
    source_file,
};
/// Shared JSON utilities: deep merge and streaming.
pub mod json;

/// FUSE filesystem implementation bridging kernel requests to the dispatch layer.
pub(crate) mod fuse;
/// Virtual node types representing files and directories in the VFS.
pub mod node;
/// Plugin registration and lifecycle management.
pub mod plugin;
/// Common re-exports for convenient use across the crate.
pub mod prelude;
/// Process spawning utilities for subprocess lifecycle management.
pub mod process;
/// Provider trait and types for contributing virtual filesystem content.
pub mod provider;
/// Built-in provider implementations (companion, agent files).
pub(crate) mod providers;
/// Linux namespace sandbox for isolating daemon subprocesses.
pub(crate) mod sandbox;
/// Session management: mount lifecycle and control socket handling.
pub(crate) mod session;
/// MiniJinja template rendering for virtual file content.
pub mod templates;
/// Shared domain types: VFS paths, file metadata, and identifiers.
pub mod types;
/// Filesystem watcher for real-FS change detection and cache invalidation.
pub(crate) mod watcher;

/// Shared test fixtures, stubs, and helpers for unit tests.
#[cfg(test)]
pub(crate) mod test_support;

// Public API: only what external consumers (CLI, provider authors) need.
pub use dispatch::activation::ActivationContext;
pub use dispatch::context::{PipelineContext, RenameContext, RequestContext};
pub use dispatch::invalidation::{EventSink, InvalidationEvent, KernelNotifier};
pub use dispatch::resolver::Resolver;
pub use dispatch::{BufferedEventSink, LoggingEventSink, ProviderRegistry, Router, WriteMode};
pub use fuse::{AsyncNotifier, FuseNotifier, NyneFs};
pub use node::builtins::{PassthroughContent, StaticContent};
pub use node::capabilities::{Lifecycle, Readable, Renameable, Unlinkable, Writable, Xattrable};
pub use node::kind::{NodeAttr, NodeKind, WriteOutcome};
pub use node::middleware::{PostWriteHook, ReadMiddleware, WriteMiddleware};
pub use node::{CachePolicy, VirtualNode};
pub use plugin::{PLUGINS, Plugin};
pub use provider::{ConflictInfo, ConflictParty, ConflictResolution, Node, Nodes, Provider, ProviderId};
pub use sandbox::{ClonerFactory, PROJECT_CLONERS, ProjectCloner};
pub use types::{
    ExtensionCounts, FileKind, GitDirName, OsFs, PassthroughProcesses, ProcessVisibility, RealFs, TypeMap, VfsPath,
};
