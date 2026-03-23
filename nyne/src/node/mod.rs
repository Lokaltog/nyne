//! Virtual filesystem node types and capabilities.

pub mod builtins;
pub mod capabilities;
pub mod diff_action;
pub mod kind;
pub(crate) mod line_slice;
pub mod middleware;
pub(crate) mod plugin;
pub(crate) mod visibility;

use std::io::{self, ErrorKind};
use std::path::PathBuf;

pub use builtins::{PassthroughContent, StaticContent};
pub use capabilities::{Lifecycle, Readable, Renameable, Unlinkable, Writable, Xattrable};
pub use kind::{NodeAttr, NodeKind, WriteOutcome};
use middleware::{ReadMiddleware, WriteMiddleware};
pub(crate) use plugin::NodePlugin;
pub(crate) use visibility::Visibility;

use crate::types::TypeMap;
use crate::types::vfs_path::VfsPath;

/// Controls whether the L2 content cache stores this node's read output.
///
/// This affects the dispatch-layer L2 cache **only**. The FUSE kernel
/// attr/entry cache TTL is determined structurally: derived inodes (those
/// with a [`source`](VirtualNode::source)) and shadow inodes (those that
/// won force-resolution over a real file) always get TTL=0 regardless of
/// this policy.
///
/// Virtual nodes default to [`CachePolicy::Cache`] — content is generated
/// once and served from the daemon's L2 cache until explicitly invalidated
/// (via [`FileGenerations`](crate::dispatch::content_cache::FileGenerations)
/// staleness or direct eviction).
///
/// Nodes whose content depends on external mutable state that changes
/// outside the FUSE write pipeline (e.g., `git status`) should use
/// [`CachePolicy::Never`] so every read re-executes the content generator.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CachePolicy {
    /// Content is cached indefinitely until explicit invalidation (default).
    #[default]
    Cache,
    /// Content is never cached — every read executes the full pipeline.
    Never,
}

/// A virtual node in the FUSE filesystem.
///
/// Identity is data (name, kind, permissions). Behavior is composed from
/// optional capability trait objects. Permissions are auto-derived from
/// attached capabilities when not explicitly set.
#[must_use]
pub struct VirtualNode {
    name: String,
    kind: NodeKind,
    permissions: Option<u16>,
    cache_policy: CachePolicy,
    visibility: Visibility,

    /// When `true`, this node shadows a real filesystem entry (won via
    /// force-resolution). The kernel cache must be bypassed (TTL=0)
    /// because visibility demoting is per-process while the kernel
    /// cache is per-inode.
    shadows_real: bool,

    /// When `true`, the dispatch layer never caches the resolved state
    /// of this directory's contents. Every readdir triggers a fresh
    /// provider resolve cycle. Used for dynamic directories whose
    /// contents depend on external state (e.g., LSP workspace search).
    volatile: bool,

    // Capabilities
    readable: Option<Box<dyn Readable>>,
    writable: Option<Box<dyn Writable>>,
    renameable: Option<Box<dyn Renameable>>,
    unlinkable: Option<Box<dyn Unlinkable>>,
    lifecycle: Option<Box<dyn Lifecycle>>,
    xattrable: Option<Box<dyn Xattrable>>,

    // Pipeline extension
    read_middlewares: Vec<Box<dyn ReadMiddleware>>,
    write_middlewares: Vec<Box<dyn WriteMiddleware>>,

    // Plugin-based parametric derivation
    plugins: Vec<Box<dyn NodePlugin>>,

    // Typed property bag for provider-specific extensions
    props: TypeMap,

    // Source file staleness tracking — providers stamp companion nodes
    // with the source file and its generation at creation time.
    source: Option<(VfsPath, u64)>,
}

/// Default permissions auto-derived from node kind and capabilities.
///
/// Used when no explicit `with_permissions()` override is set.
/// Follows standard Unix conventions for virtual filesystems.
pub(crate) mod default_permissions {
    /// Read-only directory (no writable capability).
    pub const DIR_RO: u16 = 0o500;
    /// Read-write directory (has writable capability).
    pub const DIR_RW: u16 = 0o700;
    /// Read-only file.
    pub const FILE_RO: u16 = 0o400;
    /// Read-write file.
    pub const FILE_RW: u16 = 0o600;
    /// Write-only file (no readable capability).
    pub const FILE_WO: u16 = 0o200;
    /// No capabilities attached.
    pub const FILE_NONE: u16 = 0o000;
    /// Symlinks are always permissive (target controls access).
    pub const SYMLINK: u16 = 0o700;
}

impl VirtualNode {
    /// Base constructor — all public constructors delegate here.
    fn new(name: impl Into<String>, kind: NodeKind) -> Self {
        let name = name.into();
        debug_assert!(!name.is_empty(), "VirtualNode name must not be empty");
        Self {
            name,
            kind,
            permissions: None,
            cache_policy: CachePolicy::default(),
            visibility: Visibility::default(),
            shadows_real: false,
            volatile: false,
            readable: None,
            writable: None,
            renameable: None,
            unlinkable: None,
            lifecycle: None,
            xattrable: None,
            read_middlewares: Vec::new(),
            write_middlewares: Vec::new(),
            plugins: Vec::new(),
            props: TypeMap::new(),
            source: None,
        }
    }

    /// Create a read-only file node.
    pub fn file(name: impl Into<String>, readable: impl Readable + 'static) -> Self {
        let mut node = Self::new(name, NodeKind::File { size_hint: None });
        node.readable = Some(Box::new(readable));
        node
    }

    /// Create a directory node (no capabilities needed).
    pub fn directory(name: impl Into<String>) -> Self { Self::new(name, NodeKind::Directory) }

    /// Create a symlink node.
    pub fn symlink(name: impl Into<String>, target: PathBuf) -> Self { Self::new(name, NodeKind::Symlink { target }) }

    /// Attach write capability.
    pub fn with_writable(mut self, writable: impl Writable + 'static) -> Self {
        self.writable = Some(Box::new(writable));
        self
    }

    /// Attach rename capability.
    pub fn with_renameable(mut self, renameable: impl Renameable + 'static) -> Self {
        self.renameable = Some(Box::new(renameable));
        self
    }

    /// Attach unlink capability.
    pub fn with_unlinkable(mut self, unlinkable: impl Unlinkable + 'static) -> Self {
        self.unlinkable = Some(Box::new(unlinkable));
        self
    }

    /// Attach lifecycle hooks.
    pub fn with_lifecycle(mut self, lifecycle: impl Lifecycle + 'static) -> Self {
        self.lifecycle = Some(Box::new(lifecycle));
        self
    }

    /// Attach extended attribute capability.
    pub fn with_xattrable(mut self, xattrable: impl Xattrable + 'static) -> Self {
        self.xattrable = Some(Box::new(xattrable));
        self
    }

    /// Override auto-derived permissions.
    pub const fn with_permissions(mut self, permissions: u16) -> Self {
        self.permissions = Some(permissions);
        self
    }

    /// Set the size hint for file nodes.
    pub const fn with_size_hint(mut self, size: u64) -> Self {
        if let NodeKind::File { ref mut size_hint, .. } = self.kind {
            *size_hint = Some(size);
        }
        self
    }

    /// Set the L2 content cache policy for this node.
    ///
    /// Defaults to [`CachePolicy::Cache`]. Use [`CachePolicy::Never`] for
    /// nodes whose content depends on external mutable state that changes
    /// outside the FUSE write pipeline (e.g., git status, system info).
    ///
    /// This does **not** affect the FUSE kernel cache TTL — that is
    /// determined structurally from [`source`](Self::source) and
    /// [`shadows_real`](Self::shadows_real).
    pub const fn with_cache_policy(mut self, policy: CachePolicy) -> Self {
        self.cache_policy = policy;
        self
    }

    /// Set cache policy to Never — content re-computed on every read.
    pub const fn no_cache(mut self) -> Self {
        self.cache_policy = CachePolicy::Never;
        self
    }

    /// Associate this node with a source file for staleness tracking.
    ///
    /// Companion-namespace providers stamp their nodes with the real file
    /// this node's content derives from and the file's current generation.
    /// The dispatch layer uses this to detect stale nodes on read and
    /// trigger re-resolution when the source file has been modified.
    pub fn with_source(mut self, file: VfsPath, generation: u64) -> Self {
        self.source = Some((file, generation));
        self
    }

    /// Mark this node as hidden from directory listings.
    /// Hidden nodes are still accessible by name (FUSE lookup).
    pub const fn hidden(mut self) -> Self {
        self.visibility = Visibility::Hidden;
        self
    }

    /// Mark this directory as volatile — the dispatch layer will never
    /// cache its resolved contents. Every readdir triggers a fresh
    /// provider resolve cycle.
    ///
    /// Use for directories whose contents depend on external state
    /// (e.g., LSP queries) that can change between accesses.
    pub const fn volatile(mut self) -> Self {
        self.volatile = true;
        self
    }

    /// Mark this node as shadowing a real filesystem entry.
    ///
    /// Set by the conflict resolution system when a provider Force-wins
    /// over a real file. Causes the kernel cache to be bypassed (TTL=0)
    /// because visibility demoting is per-process while the kernel cache
    /// is per-inode.
    pub(crate) const fn mark_shadows_real(mut self) -> Self {
        self.shadows_real = true;
        self
    }

    /// Get this node's visibility setting.
    pub const fn visibility(&self) -> Visibility { self.visibility }

    /// Attach a plugin to this node for parametric derivation.
    pub fn plugin(mut self, p: impl NodePlugin + 'static) -> Self {
        self.plugins.push(Box::new(p));
        self
    }

    /// Attach the `LineSlice` plugin — enables `:M-N` line slicing.
    pub fn sliceable(self) -> Self { self.plugin(line_slice::LineSlice) }

    /// Get attached plugins.
    pub fn plugins(&self) -> &[Box<dyn NodePlugin>] { &self.plugins }

    /// Check if this node has any plugins.
    pub fn has_plugins(&self) -> bool { !self.plugins.is_empty() }

    /// Attach read middlewares.
    pub fn with_read_middlewares(mut self, middlewares: Vec<Box<dyn ReadMiddleware>>) -> Self {
        self.read_middlewares = middlewares;
        self
    }

    /// Attach write middlewares.
    pub fn with_write_middlewares(mut self, middlewares: Vec<Box<dyn WriteMiddleware>>) -> Self {
        self.write_middlewares = middlewares;
        self
    }

    /// Attach a typed property accessible via [`get_prop()`](Self::get_prop).
    ///
    /// Providers attach domain-specific data (e.g., symbol locations) that
    /// other providers can read through the resolver. Uses `TypeId` as key —
    /// each type can have at most one value.
    pub fn prop<T: Send + Sync + 'static>(mut self, value: T) -> Self {
        self.props.insert(value);
        self
    }

    /// Retrieve a typed property by type.
    pub fn get_prop<T: 'static>(&self) -> Option<&T> { self.props.get::<T>() }

    pub fn name(&self) -> &str { &self.name }

    pub const fn kind(&self) -> &NodeKind { &self.kind }

    pub const fn cache_policy(&self) -> CachePolicy { self.cache_policy }

    /// The source file and generation this node's content derives from.
    ///
    /// Returns `None` for nodes not associated with a specific source file
    /// (e.g., `@/git/status`, `@/nyne/`). Companion-namespace nodes return
    /// `Some((source_file, generation_at_creation))`.
    pub fn source(&self) -> Option<(&VfsPath, u64)> { self.source.as_ref().map(|(f, g)| (f, *g)) }

    /// Whether this node shadows a real filesystem entry.
    pub(crate) const fn shadows_real(&self) -> bool { self.shadows_real }

    /// Whether this directory's contents should never be cached.
    pub const fn is_volatile(&self) -> bool { self.volatile }

    /// Permissions: explicit override, or auto-derived from capabilities.
    pub fn permissions(&self) -> u16 {
        if let Some(p) = self.permissions {
            return p;
        }
        match self.kind {
            NodeKind::Directory =>
                if self.writable.is_some() {
                    default_permissions::DIR_RW
                } else {
                    default_permissions::DIR_RO
                },
            NodeKind::Symlink { .. } => default_permissions::SYMLINK,
            NodeKind::File { .. } => match (self.readable.is_some(), self.writable.is_some()) {
                (true, true) => default_permissions::FILE_RW,
                (true, false) => default_permissions::FILE_RO,
                (false, true) => default_permissions::FILE_WO,
                (false, false) => default_permissions::FILE_NONE,
            },
        }
    }

    pub fn readable(&self) -> Option<&dyn Readable> { self.readable.as_deref() }

    pub fn writable(&self) -> Option<&dyn Writable> { self.writable.as_deref() }

    pub fn renameable(&self) -> Option<&dyn Renameable> { self.renameable.as_deref() }

    pub fn unlinkable(&self) -> Option<&dyn Unlinkable> { self.unlinkable.as_deref() }

    pub fn lifecycle(&self) -> Option<&dyn Lifecycle> { self.lifecycle.as_deref() }

    pub fn xattrable(&self) -> Option<&dyn Xattrable> { self.xattrable.as_deref() }

    pub fn read_middlewares(&self) -> &[Box<dyn ReadMiddleware>] { &self.read_middlewares }

    pub fn write_middlewares(&self) -> &[Box<dyn WriteMiddleware>] { &self.write_middlewares }
}

/// Generate a `require_*` method that returns the capability or `PermissionDenied`.
macro_rules! require_capability {
    ($method:ident, $accessor:ident, $Trait:ident, $label:expr) => {
        impl VirtualNode {
            pub fn $method(&self) -> io::Result<&dyn $Trait> {
                self.$accessor().ok_or_else(|| {
                    io::Error::new(
                        ErrorKind::PermissionDenied,
                        format!("node \"{}\" is not {}", self.name, $label),
                    )
                })
            }
        }
    };
}

require_capability!(require_readable, readable, Readable, "readable");
require_capability!(require_writable, writable, Writable, "writable");
require_capability!(require_renameable, renameable, Renameable, "renameable");
require_capability!(require_unlinkable, unlinkable, Unlinkable, "unlinkable");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::vfs_path::VfsPath;

    #[test]
    fn source_defaults_to_none() {
        let node = VirtualNode::file("test.rs", StaticContent(b""));
        assert!(node.source().is_none());
    }

    #[test]
    fn with_source_round_trip() {
        let path = VfsPath::new("src/lib.rs").unwrap();
        let node = VirtualNode::file("body.rs", StaticContent(b"fn main() {}")).with_source(path.clone(), 42);
        let (source_file, generation) = node.source().unwrap();
        assert_eq!(source_file, &path);
        assert_eq!(generation, 42);
    }

    #[test]
    fn with_source_preserves_other_fields() {
        let path = VfsPath::new("src/lib.rs").unwrap();
        let node = VirtualNode::directory("symbols").hidden().with_source(path, 1);
        assert!(node.source().is_some());
        assert_eq!(node.visibility(), Visibility::Hidden);
        assert_eq!(node.name(), "symbols");
    }
}

#[test]
fn volatile_defaults_to_false() {
    let node = VirtualNode::directory("search");
    assert!(!node.is_volatile());
}

#[test]
fn volatile_builder_sets_flag() {
    let node = VirtualNode::directory("query").no_cache().volatile();
    assert!(node.is_volatile());
    assert_eq!(node.cache_policy(), CachePolicy::Never);
}
