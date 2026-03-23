//! Nyne — expose source code as a FUSE filesystem.
extern crate self as nyne;

pub(crate) mod err;

pub mod cli;
pub mod config;
pub mod dispatch;
pub mod format;
// Re-export provider helpers for plugin crates.
pub use providers::{
    companion_children, companion_dir, companion_lookup, companion_symbol_path, dispatch_children, dispatch_lookup,
    is_file_companion, source_file,
};
pub mod json;

pub(crate) mod fuse;
pub mod node;
pub mod plugin;
pub mod prelude;
pub mod process;
pub mod provider;
pub(crate) mod providers;
pub(crate) mod sandbox;
pub(crate) mod session;
pub mod templates;
pub mod types;
pub(crate) mod watcher;

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
pub use types::{
    ExtensionCounts, FileKind, GitDirName, OsFs, PassthroughProcesses, ProcessVisibility, RealFs, TypeMap, VfsPath,
};
