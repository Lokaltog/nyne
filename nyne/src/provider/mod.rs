//! Provider interface and registration.
//!
//! Defines the [`Provider`] trait that all FUSE content providers implement,
//! along with supporting types for conflict resolution, mutation handling,
//! and provider identification. This is a Tier 2 module -- imported by both
//! core providers (`providers/`) and plugin providers (`plugins/`).
//!
//! Providers are the extensibility mechanism: each one contributes virtual
//! nodes to the VFS tree. The dispatch layer queries all active providers
//! on cache misses and merges their results.

use std::sync::Arc;
use std::{fmt, iter};

use color_eyre::eyre::Result;

use crate::dispatch::activation::ActivationContext;
use crate::dispatch::context::RequestContext;
use crate::dispatch::invalidation::InvalidationEvent;
use crate::node::VirtualNode;
use crate::node::middleware::{ReadMiddleware, WriteMiddleware};
use crate::types::real_fs::RealFs;
use crate::types::vfs_path::VfsPath;

/// A filesystem mutation operation on a real file.
///
/// Passed to [`Provider::handle_mutation`] so providers can intercept
/// real-file mutations (e.g., update git index on rename, stage deletions
/// on unlink).
///
/// When no provider claims the operation (returns [`MutationOutcome::Handled`]),
/// the router falls back to the corresponding [`RealFs`](crate::types::real_fs::RealFs)
/// method. Either way, the actual filesystem change triggers inotify events
/// that flow through the watcher pipeline for cache invalidation.
#[derive(Debug)]
pub enum MutationOp<'a> {
    /// File or directory rename/move.
    Rename { from: &'a VfsPath, to: &'a VfsPath },
    /// File deletion (FUSE `unlink`).
    Unlink { path: &'a VfsPath },
    /// Directory removal (FUSE `rmdir`).
    Rmdir { path: &'a VfsPath },
    /// File creation (FUSE `create`).
    Create { path: &'a VfsPath },
    /// Directory creation (FUSE `mkdir`).
    Mkdir { path: &'a VfsPath },
}
/// Execution methods for filesystem mutation operations.
impl MutationOp<'_> {
    /// Execute this mutation against a real filesystem.
    ///
    /// Bridges each variant to the corresponding [`RealFs`] method.
    pub(crate) fn execute(&self, fs: &dyn RealFs) -> Result<()> {
        match self {
            Self::Rename { from, to } => fs.rename(from, to),
            Self::Unlink { path } => fs.unlink(path),
            Self::Rmdir { path } => fs.rmdir(path),
            Self::Create { path } => fs.create_file(path),
            Self::Mkdir { path } => fs.mkdir(path),
        }
    }
}

/// Unique identifier for a provider.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderId(&'static str);

/// Construction and access for provider identifiers.
impl ProviderId {
    /// Create a new provider identifier.
    ///
    /// # Panics (debug builds)
    ///
    /// Panics if `id` is empty.
    pub const fn new(id: &'static str) -> Self {
        debug_assert!(!id.is_empty(), "ProviderId must not be empty");
        Self(id)
    }

    /// Return the identifier as a string slice.
    pub const fn as_str(&self) -> &'static str { self.0 }
}

/// Display implementation for [`ProviderId`].
impl fmt::Display for ProviderId {
    /// Writes the provider identifier string.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(self.0) }
}

/// Identifies the other party in a naming conflict.
#[derive(Debug, Clone)]
pub enum ConflictParty {
    /// Another provider claims the same name.
    Provider(ProviderId),
    /// A real filesystem entry exists with the same name.
    RealFile,
}

/// Describes a naming conflict between providers for a single node name.
#[derive(Debug)]
#[non_exhaustive]
pub struct ConflictInfo {
    name: String,
    party: ConflictParty,
}

impl ConflictInfo {
    /// The conflicting node name.
    pub fn name(&self) -> &str { &self.name }

    /// The other party in the conflict.
    pub const fn party(&self) -> &ConflictParty { &self.party }

    /// Build conflict infos for provider-vs-provider conflicts.
    pub fn for_providers(name: &str, pids: impl IntoIterator<Item = ProviderId>) -> Vec<Self> {
        pids.into_iter()
            .map(|pid| Self {
                name: name.to_owned(),
                party: ConflictParty::Provider(pid),
            })
            .collect()
    }

    /// Build conflict infos for real-vs-virtual conflicts (real file + providers).
    pub fn for_real_conflict(name: &str, pids: impl IntoIterator<Item = ProviderId>) -> Vec<Self> {
        iter::once(ConflictParty::RealFile)
            .chain(pids.into_iter().map(ConflictParty::Provider))
            .map(|party| Self {
                name: name.to_owned(),
                party,
            })
            .collect()
    }
}

/// Result of conflict resolution by a provider.
///
/// Models NixOS-style priority resolution: providers can yield, force-win,
/// or retry with adjusted names.
#[non_exhaustive]
pub enum ConflictResolution {
    /// Drop this provider's conflicting nodes (default).
    Yield,
    /// This provider wins — use these nodes unconditionally.
    /// If multiple providers Force for the same name, ALL are dropped (tied conflict).
    Force(Vec<VirtualNode>),
    /// Retry with adjusted names (e.g., rename to avoid collision).
    Retry(Vec<VirtualNode>),
}
impl fmt::Debug for ConflictResolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Yield => f.write_str("Yield"),
            Self::Force(nodes) => f.debug_tuple("Force").field(&nodes.len()).finish(),
            Self::Retry(nodes) => f.debug_tuple("Retry").field(&nodes.len()).finish(),
        }
    }
}

/// Outcome of a provider's [`Provider::handle_mutation`] attempt.
#[derive(Debug)]
pub enum MutationOutcome {
    /// Provider handled the mutation (filesystem + bookkeeping).
    Handled,
    /// Provider does not handle this mutation.
    NotHandled,
}

/// Return type for [`Provider::children`] — multiple nodes or nothing.
pub type Nodes = Result<Option<Vec<VirtualNode>>>;

/// Return type for [`Provider::lookup`], [`Provider::create`], [`Provider::mkdir`] -- one node or nothing.
///
/// `Ok(None)` means "this provider does not handle the requested name" and the
/// router will try other providers or fall through to the real filesystem.
/// `Ok(Some(node))` claims the name -- at most one provider may claim a given name.
pub type Node = Result<Option<VirtualNode>>;

/// Trait for FUSE content providers.
///
/// Each provider contributes virtual nodes to the VFS tree. The dispatch layer
/// queries all active providers on L1 cache misses and merges their results
/// into a unified directory listing. Providers are constructed during
/// activation and stored as `Arc<dyn Provider>` for the lifetime of the mount.
///
/// # Lifecycle
///
/// 1. **Activation**: [`should_activate`](Self::should_activate) is called once
///    at mount time. Providers that return `false` are discarded.
/// 2. **Resolution**: [`children`](Self::children) and [`lookup`](Self::lookup)
///    are called on every L1 cache miss. Methods take `self: Arc<Self>` so
///    providers can cheaply clone themselves into closures attached to nodes.
/// 3. **Mutation**: [`handle_mutation`](Self::handle_mutation) and
///    [`on_fs_change`](Self::on_fs_change) are called when the real filesystem
///    changes, giving providers a chance to update external state and request
///    cache invalidation of derived virtual content.
///
/// # Implementor notes
///
/// - Only [`id`](Self::id) and [`children`](Self::children) are required.
///   All other methods have sensible defaults (decline/no-op).
/// - Provider structs should be `pub(crate)` -- the dispatch layer only
///   sees `Arc<dyn Provider>`.
pub trait Provider: Send + Sync {
    /// Unique identifier for this provider.
    fn id(&self) -> ProviderId;

    /// Whether this provider should activate for the given project.
    /// Defaults to `true` (always active).
    ///
    /// Called during construction — use this for cheap checks (e.g.,
    /// "is there a `.git/` directory?"). The `ActivationContext` provides
    /// access to shared resources so providers don't need to duplicate
    /// discovery logic.
    fn should_activate(&self, _ctx: &ActivationContext) -> bool { true }

    /// Return all known children for a directory path (visible + hidden).
    ///
    /// Called on L1 cache miss for a directory. Return `None` if this provider
    /// has nothing for this directory. All providers are queried; results merged.
    /// Visibility is controlled per-node via `.hidden()`, not by method choice.
    fn children(self: Arc<Self>, ctx: &RequestContext<'_>) -> Nodes;

    /// Look up a specific name not found in `children()` results.
    ///
    /// Called as fallback when a FUSE lookup finds no match in L1 cache
    /// after `children()` has populated it. Used for hidden entries and
    /// parameterized entries. Nodes returned here are NEVER visible in
    /// directory listings. At most one provider may claim a given name.
    fn lookup(self: Arc<Self>, _ctx: &RequestContext<'_>, _name: &str) -> Node { Ok(None) }

    /// Conflict resolution for `children()` name collisions.
    ///
    /// Called when two providers emit the same node name via `children()`.
    /// Return a [`ConflictResolution`] to indicate how to handle the conflict.
    fn on_conflict(
        self: Arc<Self>,
        _ctx: &RequestContext<'_>,
        _conflicts: &[ConflictInfo],
    ) -> Result<ConflictResolution> {
        Ok(ConflictResolution::Yield)
    }

    /// Handle file creation in a directory this provider contributes to.
    ///
    /// Called when FUSE receives `create()` for a name that doesn't exist.
    /// Uses single-claim semantics like [`lookup`](Self::lookup): at most one provider may
    /// claim a given name. Return `None` to decline.
    fn create(self: Arc<Self>, _ctx: &RequestContext<'_>, _name: &str) -> Node { Ok(None) }

    /// Handle directory creation in a directory this provider contributes to.
    ///
    /// Called when FUSE receives `mkdir()` for a name that doesn't exist.
    /// Uses single-claim semantics like [`create`](Self::create): at most one provider may
    /// claim a given name. Return `None` to decline.
    fn mkdir(self: Arc<Self>, _ctx: &RequestContext<'_>, _name: &str) -> Node { Ok(None) }

    /// Handle a real-file mutation via the overlay filesystem.
    ///
    /// Providers that manage external state (e.g., git index) can claim
    /// a mutation by performing the filesystem operation through `real_fs`
    /// and any necessary bookkeeping, then returning `Handled`.
    ///
    /// **Back-propagation pattern:** the filesystem mutation is performed
    /// on the overlay merged view (via `OsFs`). This triggers inotify
    /// events on the overlay root, which the watcher
    /// converts to `VfsPath` changes and feeds back into the router for
    /// cache invalidation. The FUSE layer then reflects the new state
    /// on next access — no manual cache manipulation needed.
    ///
    /// Return `NotHandled` to decline — the router falls back to the
    /// corresponding [`RealFs`] method for the operation.
    fn handle_mutation(&self, _op: &MutationOp<'_>, _real_fs: &dyn RealFs) -> Result<MutationOutcome> {
        Ok(MutationOutcome::NotHandled)
    }

    /// Default write middlewares for this provider's nodes.
    /// Applied after node-specific middlewares, before global middlewares.
    fn write_middlewares(&self) -> Vec<Box<dyn WriteMiddleware>> { Vec::new() }

    /// Called when real filesystem changes are detected by the watcher.
    ///
    /// `changed` contains the [`VfsPath`]s of files/directories that were
    /// created, modified, or removed on the real filesystem. Providers can
    /// return [`InvalidationEvent`]s to request cache invalidation of their
    /// virtual content (e.g., invalidate `@/git/branches/` when `.git/refs/`
    /// changes).
    ///
    /// These events originate from two sources:
    /// - **External changes** — the user or another tool modifies files
    ///   outside nyne (e.g., `git commit`, editor save).
    /// - **Back-propagated provider actions** — when a provider mutates
    ///   the real filesystem via `real_fs` (e.g., `handle_mutation`), those
    ///   mutations trigger inotify events that flow back through the
    ///   watcher into this callback. This is the intended propagation
    ///   path — providers perform real-FS mutations and let the watcher
    ///   pipeline handle FUSE cache invalidation automatically.
    ///
    /// The router handles general cache invalidation (L1/L2/kernel) for
    /// the changed real-FS paths before calling this method. Providers
    /// only need to return events here for *derived* virtual content
    /// that maps to different VFS paths than the changed real paths.
    fn on_fs_change(&self, _changed: &[VfsPath]) -> Vec<InvalidationEvent> { Vec::new() }

    /// Default read middlewares for this provider's nodes.
    /// Applied after node-specific middlewares, before global middlewares.
    fn read_middlewares(&self) -> Vec<Box<dyn ReadMiddleware>> { Vec::new() }
}
