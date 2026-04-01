use std::path::{Path, PathBuf};

use anymap2::Map;
use anymap2::any::CloneAnySendSync;

use crate::router::node::NodeAccumulator;

/// The process that initiated a filesystem request.
///
/// Set by the backend (e.g., FUSE populates from the kernel request).
/// Middleware like [`VisibilityProvider`] uses this to apply per-process
/// policies. `None` when the backend has no process context (e.g., tests).
#[derive(Debug, Clone)]
pub struct Process {
    /// OS process ID.
    pub pid: u32,
    /// Process name from `/proc/{pid}/comm` (may be truncated to 15 bytes).
    pub name: Option<String>,
}

/// An operation being dispatched through the middleware chain.
#[derive(Debug, Clone)]
pub enum Op {
    /// List all children of a directory.
    Readdir,
    /// Look up a specific name in a directory.
    Lookup { name: String },
    /// Create a file.
    Create { name: String },
    /// Create a directory.
    Mkdir { name: String },
    /// Remove a file or directory.
    Remove { name: String },
    /// Rename/move a node.
    Rename {
        src_name: String,
        target_dir: PathBuf,
        target_name: String,
    },
}

impl Op {
    /// Whether this is a read-only resolution op (`Readdir` or `Lookup`).
    ///
    /// Query ops produce directory listings; mutation ops modify the tree.
    pub const fn is_query(&self) -> bool { matches!(self, Self::Readdir | Self::Lookup { .. }) }

    /// Whether this is a mutation op (`Create`, `Mkdir`, `Remove`, `Rename`).
    pub const fn is_mutation(&self) -> bool { !self.is_query() }

    /// Whether this is a `Readdir` op.
    pub const fn is_readdir(&self) -> bool { matches!(self, Self::Readdir) }

    /// Whether this is a `Create` op.
    pub const fn is_create(&self) -> bool { matches!(self, Self::Create { .. }) }

    /// Whether this is a `Remove` op.
    pub const fn is_remove(&self) -> bool { matches!(self, Self::Remove { .. }) }

    /// Whether this is a `Rename` op.
    pub const fn is_rename(&self) -> bool { matches!(self, Self::Rename { .. }) }

    /// The looked-up name, if this is a `Lookup` op.
    pub fn lookup_name(&self) -> Option<&str> {
        match self {
            Self::Lookup { name } => Some(name),
            _ => None,
        }
    }

    /// The target entry name for ops that operate on a named child.
    ///
    /// Returns `None` for `Readdir` (which lists all children).
    /// For `Rename`, returns the source name.
    pub fn target_name(&self) -> Option<&str> {
        match self {
            Self::Readdir => None,
            Self::Lookup { name } | Self::Create { name } | Self::Mkdir { name } | Self::Remove { name } => Some(name),
            Self::Rename { src_name, .. } => Some(src_name),
        }
    }
}

/// State type that preserves the original path before any rewrites.
#[derive(Debug, Clone)]
struct OriginalPath(PathBuf);

/// Cloneable snapshot of request state, for cache middleware to store
/// and restore cross-provider state alongside cached nodes.
pub type StateSnapshot = Map<dyn CloneAnySendSync + Send + Sync>;

/// Mutable context flowing through the middleware chain.
pub struct Request {
    /// Directory path being operated on.
    path: PathBuf,

    /// The operation.
    op: Op,

    /// The requesting process (populated by the backend, `None` in tests).
    process: Option<Process>,

    /// Accumulated virtual nodes.
    pub nodes: NodeAccumulator,

    /// Typed state map for cross-provider communication.
    /// Uses `CloneAnySendSync` so the cache middleware can snapshot and
    /// restore state alongside cached nodes.
    state: StateSnapshot,
}

impl Request {
    pub fn new(path: PathBuf, op: Op) -> Self {
        Self {
            path,
            op,
            process: None,
            nodes: NodeAccumulator::new(),
            state: Map::new(),
        }
    }

    /// Set the requesting process identity.
    #[must_use]
    pub fn with_process(mut self, process: Process) -> Self {
        self.process = Some(process);
        self
    }

    /// The requesting process, if known.
    pub const fn process(&self) -> Option<&Process> { self.process.as_ref() }

    /// Current directory path (may have been rewritten by upstream middleware).
    pub fn path(&self) -> &Path { &self.path }

    /// Current operation (may have been rewritten by upstream middleware).
    pub const fn op(&self) -> &Op { &self.op }

    /// Rewrite the path for downstream providers.
    /// The original path is preserved and accessible via `original_path()`.
    pub fn rewrite_path(&mut self, new_path: PathBuf) {
        if !self.state.contains::<OriginalPath>() {
            self.state.insert(OriginalPath(self.path.clone()));
        }
        self.path = new_path;
    }

    /// Rewrite the operation for downstream providers.
    pub fn set_op(&mut self, op: Op) { self.op = op; }

    /// Get the original path before any rewrites.
    pub fn original_path(&self) -> &Path {
        self.state
            .get::<OriginalPath>()
            .map_or(self.path.as_path(), |p| p.0.as_path())
    }

    /// Insert typed state for downstream providers to read.
    pub fn set_state<T: Clone + Send + Sync + 'static>(&mut self, value: T) { self.state.insert(value); }

    /// Read typed state set by an upstream provider.
    pub fn state<T: Clone + Send + Sync + 'static>(&self) -> Option<&T> { self.state.get::<T>() }

    /// Clone the entire state map (for cache snapshotting).
    pub fn clone_state(&self) -> StateSnapshot { self.state.clone() }

    /// Replace the state map with a previously cloned snapshot.
    pub fn restore_state(&mut self, state: StateSnapshot) { self.state = state; }
}
