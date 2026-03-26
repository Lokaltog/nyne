//! Virtual filesystem node types and capabilities.
//!
//! A [`VirtualNode`] is the unit of content in the FUSE tree. Identity is
//! data (name, kind, permissions), while behavior is composed from optional
//! capability trait objects ([`Readable`], [`Writable`], [`Renameable`], etc.).
//! This composition-over-inheritance design lets providers mix capabilities
//! freely without subclassing.
//!
//! Permissions are auto-derived from attached capabilities when not explicitly
//! overridden, so a node with both [`Readable`] and [`Writable`] automatically
//! gets `0o600`, while a read-only file gets `0o400`.
//!
//! Rarely-used extensions (rename, unlink, lifecycle, xattr, plugins,
//! middlewares, typed properties) are lazily allocated behind a single
//! [`Box`] to keep the common case (simple readable files) lightweight.

/// Built-in node content types (static, empty, symlink).
pub mod builtins;
/// Node capability traits (readable, writable, unlinkable, renameable).
pub mod capabilities;
/// Node kind classification (file, directory, symlink).
pub mod kind;
/// Line-range slicing plugin for `lines:M-N` suffixes.
pub(crate) mod line_slice;
/// Read/write middleware pipeline for node content transformations.
pub mod middleware;
/// Node plugin trait for extending node construction.
pub(crate) mod plugin;

use std::fmt;
use std::io::{self, ErrorKind};
use std::path::PathBuf;

pub use builtins::{PassthroughContent, StaticContent};
pub use capabilities::{Lifecycle, Readable, Renameable, Unlinkable, Writable, Xattrable};
pub use kind::{NodeAttr, NodeKind, WriteOutcome};
use middleware::{ReadMiddleware, WriteMiddleware};
pub(crate) use plugin::NodePlugin;

/// Controls whether a node appears in directory listings.
///
/// Providers set this at construction time. The dispatch layer
/// checks it during readdir to filter entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Visibility {
    /// Visible in readdir and accessible by name lookup (default).
    #[default]
    Readdir,
    /// Accessible by name lookup only — hidden from readdir listings.
    Hidden,
}

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

/// Extension capabilities and metadata — lazily allocated, only when needed.
///
/// Grouped behind a single `Box` to reduce per-node memory for the common
/// case where none of these are used (directories, symlinks, simple files).
#[derive(Default)]
struct NodeExtensions {
    renameable: Option<Box<dyn Renameable>>,
    unlinkable: Option<Box<dyn Unlinkable>>,
    lifecycle: Option<Box<dyn Lifecycle>>,
    xattrable: Option<Box<dyn Xattrable>>,
    read_middlewares: Vec<Box<dyn ReadMiddleware>>,
    write_middlewares: Vec<Box<dyn WriteMiddleware>>,
    plugins: Vec<Box<dyn NodePlugin>>,
    props: TypeMap,
}
/// Custom `Debug` to avoid requiring `Debug` bounds on capability trait objects.
///
/// Prints only presence/count information (e.g., `renameable: true`,
/// `plugins: 2`) rather than attempting to format opaque trait objects.
impl fmt::Debug for NodeExtensions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeExtensions")
            .field("renameable", &self.renameable.is_some())
            .field("unlinkable", &self.unlinkable.is_some())
            .field("lifecycle", &self.lifecycle.is_some())
            .field("xattrable", &self.xattrable.is_some())
            .field("read_middlewares", &self.read_middlewares.len())
            .field("write_middlewares", &self.write_middlewares.len())
            .field("plugins", &self.plugins.len())
            .finish_non_exhaustive()
    }
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

    // Common capabilities — kept inline (most file nodes have these)
    readable: Option<Box<dyn Readable>>,
    writable: Option<Box<dyn Writable>>,

    // Source file staleness tracking — providers stamp companion nodes
    // with the source file and its generation at creation time.
    source: Option<(VfsPath, u64)>,

    // Rarely-used extensions — lazily allocated
    ext: Option<Box<NodeExtensions>>,
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

/// Construction, capability attachment, and property access.
///
/// Uses a builder pattern: start with [`file()`](Self::file),
/// [`directory()`](Self::directory), or [`symlink()`](Self::symlink), then
/// chain `with_*` methods to attach capabilities. The builder consumes
/// `self` so nodes are fully configured before being handed to the dispatch
/// layer (which wraps them in `Arc`).
impl VirtualNode {
    /// Base constructor — all public constructors delegate here.
    ///
    /// # Panics
    ///
    /// Debug-asserts that `name` is non-empty.
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
            readable: None,
            writable: None,
            source: None,
            ext: None,
        }
    }

    /// Get or create the lazily-allocated extensions box.
    ///
    /// Extensions are behind `Option<Box<...>>` so the common case (a simple
    /// readable file) pays zero allocation cost for capabilities it never uses.
    fn ext_mut(&mut self) -> &mut NodeExtensions { self.ext.get_or_insert_with(Box::default) }

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
        self.ext_mut().renameable = Some(Box::new(renameable));
        self
    }

    /// Attach unlink capability.
    pub fn with_unlinkable(mut self, unlinkable: impl Unlinkable + 'static) -> Self {
        self.ext_mut().unlinkable = Some(Box::new(unlinkable));
        self
    }

    /// Attach lifecycle hooks.
    pub fn with_lifecycle(mut self, lifecycle: impl Lifecycle + 'static) -> Self {
        self.ext_mut().lifecycle = Some(Box::new(lifecycle));
        self
    }

    /// Attach extended attribute capability.
    pub fn with_xattrable(mut self, xattrable: impl Xattrable + 'static) -> Self {
        self.ext_mut().xattrable = Some(Box::new(xattrable));
        self
    }

    /// Override the auto-derived permission bits.
    ///
    /// By default, permissions are inferred from attached capabilities (see
    /// [`default_permissions`]). Use this when the default derivation is
    /// wrong — e.g., a file that should appear executable or a directory
    /// with non-standard access.
    pub const fn with_permissions(mut self, permissions: u16) -> Self {
        self.permissions = Some(permissions);
        self
    }

    /// Set the size hint reported in `getattr` for file nodes.
    ///
    /// Without a hint, the FUSE layer reports size 0 until the file is read.
    /// Some tools (e.g., `cat`, editors) use the reported size to allocate
    /// buffers, so a reasonable hint avoids unnecessary re-reads. Silently
    /// ignored for non-file node kinds.
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
    pub(crate) const fn visibility(&self) -> Visibility { self.visibility }

    /// Attach a plugin to this node for parametric derivation.
    pub fn plugin(mut self, p: impl NodePlugin + 'static) -> Self {
        self.ext_mut().plugins.push(Box::new(p));
        self
    }

    /// Attach the `LineSlice` plugin — enables `:M-N` line slicing.
    pub fn sliceable(self) -> Self { self.plugin(line_slice::LineSlice) }

    /// Get attached plugins.
    pub fn plugins(&self) -> &[Box<dyn NodePlugin>] { self.ext.as_ref().map_or(&[], |e| &e.plugins) }

    /// Check if this node has any plugins.
    pub fn has_plugins(&self) -> bool { self.ext.as_ref().is_some_and(|e| !e.plugins.is_empty()) }

    /// Attach read middlewares.
    pub fn with_read_middlewares(mut self, middlewares: Vec<Box<dyn ReadMiddleware>>) -> Self {
        self.ext_mut().read_middlewares = middlewares;
        self
    }

    /// Attach write middlewares.
    pub fn with_write_middlewares(mut self, middlewares: Vec<Box<dyn WriteMiddleware>>) -> Self {
        self.ext_mut().write_middlewares = middlewares;
        self
    }

    /// Attach a typed property accessible via [`get_prop()`](Self::get_prop).
    ///
    /// Providers attach domain-specific data (e.g., symbol locations) that
    /// other providers can read through the resolver. Uses `TypeId` as key —
    /// each type can have at most one value.
    pub fn prop<T: Send + Sync + 'static>(mut self, value: T) -> Self {
        self.ext_mut().props.insert(value);
        self
    }

    /// Retrieve a typed property by type.
    pub fn get_prop<T: 'static>(&self) -> Option<&T> { self.ext.as_ref()?.props.get::<T>() }

    /// Returns the node's name.
    pub fn name(&self) -> &str { &self.name }

    /// Returns the node's kind.
    pub const fn kind(&self) -> &NodeKind { &self.kind }

    /// Returns the node's cache policy.
    pub const fn cache_policy(&self) -> CachePolicy { self.cache_policy }

    /// The source file and generation this node's content derives from.
    ///
    /// Returns `None` for nodes not associated with a specific source file
    /// (e.g., `@/git/status`, `@/nyne/`). Companion-namespace nodes return
    /// `Some((source_file, generation_at_creation))`.
    pub fn source(&self) -> Option<(&VfsPath, u64)> { self.source.as_ref().map(|(f, g)| (f, *g)) }

    /// Whether this node shadows a real filesystem entry.
    pub(crate) const fn shadows_real(&self) -> bool { self.shadows_real }

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

    /// Returns the readable capability, if attached.
    pub fn readable(&self) -> Option<&dyn Readable> { self.readable.as_deref() }

    /// Returns the writable capability, if attached.
    pub fn writable(&self) -> Option<&dyn Writable> { self.writable.as_deref() }

    /// Returns the renameable capability, if attached.
    pub fn renameable(&self) -> Option<&dyn Renameable> { self.ext.as_ref()?.renameable.as_deref() }

    /// Returns the unlinkable capability, if attached.
    pub fn unlinkable(&self) -> Option<&dyn Unlinkable> { self.ext.as_ref()?.unlinkable.as_deref() }

    /// Returns the lifecycle hooks, if attached.
    pub fn lifecycle(&self) -> Option<&dyn Lifecycle> { self.ext.as_ref()?.lifecycle.as_deref() }

    /// Returns the xattr capability, if attached.
    pub fn xattrable(&self) -> Option<&dyn Xattrable> { self.ext.as_ref()?.xattrable.as_deref() }

    /// Returns the attached read middlewares.
    pub fn read_middlewares(&self) -> &[Box<dyn ReadMiddleware>] {
        self.ext.as_ref().map_or(&[], |e| &e.read_middlewares)
    }

    /// Returns the attached write middlewares.
    pub fn write_middlewares(&self) -> &[Box<dyn WriteMiddleware>] {
        self.ext.as_ref().map_or(&[], |e| &e.write_middlewares)
    }
}

/// Generate a `require_*` method that returns the capability or `PermissionDenied`.
///
/// Produces methods like `require_readable()`, `require_writable()`, etc.
/// that convert `Option<&dyn Trait>` accessors into `io::Result`, mapping
/// `None` to `PermissionDenied` with a descriptive error message. The FUSE
/// layer uses these to fail gracefully when an operation is attempted on a
/// node lacking the required capability.
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

/// Unit tests.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::vfs_path::VfsPath;

    /// Tests that source defaults to None for nodes without a source.
    #[test]
    fn source_defaults_to_none() {
        let node = VirtualNode::file("test.rs", StaticContent(b""));
        assert!(node.source().is_none());
    }

    /// Tests that with_source round-trips the source file and generation.
    #[test]
    fn with_source_round_trip() {
        let path = VfsPath::new("src/lib.rs").unwrap();
        let node = VirtualNode::file("body.rs", StaticContent(b"fn main() {}")).with_source(path.clone(), 42);
        let (source_file, generation) = node.source().unwrap();
        assert_eq!(source_file, &path);
        assert_eq!(generation, 42);
    }

    /// Tests that with_source preserves other node fields.
    #[test]
    fn with_source_preserves_other_fields() {
        let path = VfsPath::new("src/lib.rs").unwrap();
        let node = VirtualNode::directory("symbols").hidden().with_source(path, 1);
        assert!(node.source().is_some());
        assert_eq!(node.visibility(), Visibility::Hidden);
        assert_eq!(node.name(), "symbols");
    }
}
