use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use std::{fmt, mem};

use bitflags::bitflags;
use color_eyre::eyre::Result;

use crate::router::fs::Filesystem;
pub use crate::types::NodeKind;
use crate::types::Timestamps;

pub mod decorators;

/// Context passed to [`Readable::read`] when content is requested.
pub struct ReadContext<'a> {
    /// The virtual path being read.
    pub path: &'a Path,
    /// Filesystem backend for reading source files.
    pub fs: &'a dyn Filesystem,
}

/// Context passed to [`Writable::write`] when content is written.
pub struct WriteContext<'a> {
    /// The virtual path being written.
    pub path: &'a Path,
    /// Filesystem backend for reading/writing source files.
    pub fs: &'a dyn Filesystem,
}

/// Context passed to [`Renameable::rename`] for rename operations.
pub struct RenameContext<'a> {
    /// The source virtual path.
    pub source: &'a Path,
    /// The target virtual path.
    pub target: &'a Path,
}

/// Context passed to [`Unlinkable::unlink`] for delete operations.
pub struct UnlinkContext<'a> {
    /// The virtual path being unlinked.
    pub path: &'a Path,
}

/// Content reader for a virtual node.
pub trait Readable: Send + Sync {
    fn read(&self, ctx: &ReadContext<'_>) -> Result<Vec<u8>>;

    /// Optional content size without reading full content.
    /// Returns `None` if size is unknown — the filesystem backend falls back
    /// to reading content or using a backend-specific default.
    fn size(&self) -> Option<u64> { None }

    /// Path to the backing file on disk, if any.
    fn backing_path(&self) -> Option<&Path> { None }
}

/// Paths of source files affected by a write operation.
pub type AffectedFiles = Vec<PathBuf>;

/// Content writer for a virtual node.
pub trait Writable: Send + Sync {
    /// Write content and return the source files that were modified.
    ///
    /// Callers use the returned paths to invalidate caches and notify
    /// providers about changed files.
    fn write(&self, ctx: &WriteContext<'_>, data: &[u8]) -> Result<AffectedFiles>;
}

impl<T: Readable + ?Sized> Readable for Arc<T> {
    fn read(&self, ctx: &ReadContext<'_>) -> Result<Vec<u8>> { (**self).read(ctx) }

    fn size(&self) -> Option<u64> { (**self).size() }

    fn backing_path(&self) -> Option<&Path> { (**self).backing_path() }
}

impl<T: Writable + ?Sized> Writable for Arc<T> {
    fn write(&self, ctx: &WriteContext<'_>, data: &[u8]) -> Result<AffectedFiles> { (**self).write(ctx, data) }
}

/// Closure-backed [`Readable`] — captures state at dispatch time,
/// reads lazily at access time.
pub struct LazyReadable<R> {
    read_fn: R,
}

impl<R> LazyReadable<R> {
    /// Create a read-only lazy readable.
    ///
    /// The `for<'a>` bound ensures the closure satisfies the HRTB requirement
    /// at construction time, avoiding lifetime inference failures at call sites.
    pub const fn new(read_fn: R) -> Self
    where
        R: for<'a> Fn(&ReadContext<'a>) -> Result<Vec<u8>> + Send + Sync,
    {
        Self { read_fn }
    }
}

impl<R> Readable for LazyReadable<R>
where
    R: for<'a> Fn(&ReadContext<'a>) -> Result<Vec<u8>> + Send + Sync,
{
    fn read(&self, ctx: &ReadContext<'_>) -> Result<Vec<u8>> { (self.read_fn)(ctx) }
}

/// Rename handler for a virtual node.
pub trait Renameable: Send + Sync {
    fn rename(&self, ctx: &RenameContext<'_>) -> Result<AffectedFiles>;
}

/// Delete handler for a virtual node.
pub trait Unlinkable: Send + Sync {
    fn unlink(&self, ctx: &UnlinkContext<'_>) -> Result<AffectedFiles>;
}

/// Cache policy for a node — controls **both** the nyne content cache
/// (`CachedReadable` wrapping in the cache plugin) **and** the kernel's
/// dentry/attr cache TTL (FUSE `entry_valid` / `attr_valid`).
///
/// The two layers move in lockstep: opting out of content caching also
/// forces the kernel to re-resolve on every access; setting an explicit
/// TTL applies the same duration to both layers.
///
/// | Variant | Content cache | Kernel attr/entry cache |
/// |-|-|-|
/// | `Default` | Persistent (generation-invalidated) | Per-file-type heuristic (1s real, 0 virtual) |
/// | `NoCache` | Not wrapped — every read hits inner | `Duration::ZERO` — kernel re-resolves every access |
/// | `Ttl(d)` | Cached for `d`, then re-read | `d` returned to FUSE |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CachePolicy {
    /// Fall back to per-layer defaults.
    #[default]
    Default,
    /// Don't cache at either layer. Equivalent to `Ttl(Duration::ZERO)`
    /// but more legible at call sites.
    NoCache,
    /// Cache for `Duration` at both layers. `Duration::ZERO` is the
    /// same as [`CachePolicy::NoCache`].
    Ttl(Duration),
}

/// Lifecycle hooks for nodes that need to track open/close state.
/// Filesystem backends call these when handles are opened and released.
pub trait Lifecycle: Send + Sync {
    /// Called when a handle is opened for this node.
    fn on_open(&self) {}
    /// Called when all handles to this node are released.
    fn on_close(&self) {}
}

/// Key-value metadata capability for nodes.
/// Filesystem backends map this to their native attribute mechanism
/// (e.g., xattrs on FUSE, headers on HTTP).
pub trait Attributable: Send + Sync {
    fn get(&self, key: &str) -> Option<Vec<u8>>;
    fn set(&self, key: &str, value: &[u8]) -> Result<()>;
    fn list(&self) -> Vec<String>;
}

/// Declares the `Node` struct, capability accessors, constructor, `Clone`, and `Debug`
/// — all driven from a single list of `field: Trait` pairs. Adding a new capability
/// slot is a one-line change here; the macro handles struct fields, accessors,
/// constructor init, clone, debug, and slot-level merge.
///
/// Hand-written `Node` methods live in a separate `impl Node` block below the
/// invocation.
macro_rules! node_with_slots {
    ($($field:ident: $Trait:ident),+ $(,)?) => {
        /// An unnamed virtual filesystem node with optional capability slots.
        ///
        /// `Node` carries kind and capabilities but no name. Use [`Node::named`] or
        /// [`NamedNode::new`] to pair it with a name for use in [`NodeAccumulator`].
        pub struct Node {
            kind: NodeKind,
            target: Option<PathBuf>,
            permissions: Option<Permissions>,
            timestamps: Option<Timestamps>,
            cache_policy: CachePolicy,
            $($field: Option<Arc<dyn $Trait>>,)+
        }

        // Generated: slot accessors, constructor, slot-level merge helper.
        impl Node {
            $(
                pub fn $field(&self) -> Option<&dyn $Trait> { self.$field.as_deref() }

                paste::paste! {
                    pub fn [<take_ $field>](&mut self) -> Option<Arc<dyn $Trait>> {
                        self.$field.take()
                    }

                    pub fn [<set_ $field>](&mut self, val: impl $Trait + 'static) {
                        self.$field = Some(Arc::new(val));
                    }

                    #[must_use]
                    pub fn [<with_ $field>](mut self, val: impl $Trait + 'static) -> Self {
                        self.$field = Some(Arc::new(val));
                        self
                    }
                }
            )+

            fn new(kind: NodeKind) -> Self {
                Self {
                    kind,
                    target: None,
                    permissions: None,
                    timestamps: None,
                    cache_policy: CachePolicy::default(),
                    $($field: None,)+
                }
            }

            /// Merge capability slots from `other` into `self` (first-writer-wins).
            fn merge_slots(&mut self, other: &mut Self) {
                $(if self.$field.is_none() { self.$field = other.$field.take(); })+
            }
        }

        impl Clone for Node {
            fn clone(&self) -> Self {
                Self {
                    kind: self.kind,
                    target: self.target.clone(),
                    permissions: self.permissions,
                    timestamps: self.timestamps,
                    cache_policy: self.cache_policy.clone(),
                    $($field: self.$field.clone(),)+
                }
            }
        }

        impl fmt::Debug for Node {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let mut s = f.debug_struct("Node");
                s.field("kind", &self.kind);
                if let Some(target) = &self.target {
                    s.field("target", target);
                }
                s.field("permissions", &self.permissions())
                    .field("timestamps", &self.timestamps)
                    .field("cache_policy", &self.cache_policy);
                $(s.field(stringify!($field), &self.$field.is_some());)+
                s.finish()
            }
        }
    };
}

node_with_slots! {
    readable: Readable,
    writable: Writable,
    renameable: Renameable,
    unlinkable: Unlinkable,
    lifecycle: Lifecycle,
    attributable: Attributable,
}

bitflags! {
    /// Backend-agnostic permission flags for virtual nodes.
    ///
    /// Either explicitly set via [`Node::with_permissions`] or auto-derived from
    /// kind and capabilities. Use [`bits()`](Self::bits) for raw bit access.
    /// Backend-specific translation (e.g., FUSE octal modes) belongs in the
    /// backend crate via an extension trait — the router stays agnostic.
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub struct Permissions: u8 {
        const NONE = 0;
        const READ = 1 << 0;
        const WRITE = 1 << 1;
        const EXECUTE = 1 << 2;
        const ALL = Self::READ.bits() | Self::WRITE.bits() | Self::EXECUTE.bits();
    }
}

impl Permissions {
    /// Construct from raw bits, masking to the valid 3-bit range.
    ///
    /// Panics in debug if `bits` has any bits set outside the valid mask —
    /// this catches accidental upcasts from larger types. Release builds
    /// silently mask, matching the safer of the two behaviours.
    pub const fn from_bits_masked(bits: u8) -> Self {
        debug_assert!(bits <= 0b111, "Permissions::from_bits called with out-of-range value");
        Self::from_bits_truncate(bits)
    }
}

impl fmt::Debug for Permissions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "Permissions({self})") }
}

impl fmt::Display for Permissions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let flag = |has, ch| if has { ch } else { '-' };
        write!(
            f,
            "{}{}{}",
            flag(self.contains(Self::READ), 'r'),
            flag(self.contains(Self::WRITE), 'w'),
            flag(self.contains(Self::EXECUTE), 'x'),
        )
    }
}

// Hand-written Node methods — non-slot logic only.
impl Node {
    pub fn file() -> Self { Self::new(NodeKind::File) }

    pub fn dir() -> Self { Self::new(NodeKind::Directory) }

    pub fn symlink(target: impl Into<PathBuf>) -> Self {
        Self {
            target: Some(target.into()),
            ..Self::new(NodeKind::Symlink)
        }
    }

    pub const fn kind(&self) -> NodeKind { self.kind }

    pub fn target(&self) -> Option<&Path> { self.target.as_deref() }

    /// Returns the resolved permissions for this node.
    ///
    /// If explicit permissions were set via [`with_permissions`](Self::with_permissions),
    /// those are returned. Otherwise, permissions are auto-derived from the
    /// node's kind and capabilities.
    pub fn permissions(&self) -> Permissions {
        if let Some(perms) = self.permissions {
            return perms;
        }
        match self.kind {
            NodeKind::Directory => {
                let mut perms = Permissions::READ | Permissions::EXECUTE;
                if self.writable.is_some() || self.unlinkable.is_some() {
                    perms |= Permissions::WRITE;
                }
                perms
            }
            NodeKind::Symlink => Permissions::ALL,
            NodeKind::File => {
                let mut perms = Permissions::NONE;
                if self.readable.is_some() {
                    perms |= Permissions::READ;
                }
                if self.writable.is_some() {
                    perms |= Permissions::WRITE;
                }
                perms
            }
        }
    }

    /// Set explicit permissions, overriding auto-derivation.
    #[must_use]
    pub const fn with_permissions(mut self, perms: Permissions) -> Self {
        self.permissions = Some(perms);
        self
    }

    /// Returns the resolved timestamps for this node.
    ///
    /// If explicit timestamps were set via [`with_timestamps`](Self::with_timestamps),
    /// those are returned. Otherwise, returns the default (all `UNIX_EPOCH`).
    pub fn timestamps(&self) -> Timestamps { self.timestamps.unwrap_or_default() }

    /// Set explicit timestamps, overriding the default.
    #[must_use]
    pub const fn with_timestamps(mut self, ts: Timestamps) -> Self {
        self.timestamps = Some(ts);
        self
    }

    /// Set timestamps only if none were explicitly set.
    ///
    /// Used by the companion provider to inherit the source file's mtime
    /// onto all virtual nodes without overriding explicitly-set timestamps.
    pub const fn set_default_timestamps(&mut self, ts: Timestamps) {
        if self.timestamps.is_none() {
            self.timestamps = Some(ts);
        }
    }

    /// Get the cache policy. Defaults to [`CachePolicy::Default`] if never
    /// explicitly set — see [`CachePolicy`] for layer-by-layer semantics.
    pub const fn cache_policy(&self) -> CachePolicy { self.cache_policy }

    /// Set the cache policy for this node — controls both content and
    /// kernel attr/entry caching. See [`CachePolicy`] for variant semantics.
    #[must_use]
    pub const fn with_cache_policy(mut self, policy: CachePolicy) -> Self {
        self.cache_policy = policy;
        self
    }

    /// Wrap this node with a name, producing a [`NamedNode`].
    #[must_use]
    pub fn named(self, name: impl Into<String>) -> NamedNode {
        NamedNode {
            name: name.into(),
            node: self,
        }
    }

    /// Merge capabilities from another node into this one.
    /// Non-contested slots are combined. Contested slots keep the existing
    /// value (first-writer-wins — earlier provider in the chain has priority).
    ///
    /// Both nodes must have the same kind — merging a file with a directory
    /// is a bug in the provider chain.
    pub fn merge_capabilities_from(&mut self, mut other: Self) {
        debug_assert_eq!(
            self.kind, other.kind,
            "cannot merge capabilities across different node kinds"
        );
        if self.target.is_none() {
            self.target = other.target.take();
        }
        self.merge_slots(&mut other);
        if self.permissions.is_none() {
            self.permissions = other.permissions;
        }
        if self.timestamps.is_none() {
            self.timestamps = other.timestamps;
        }
        // First-writer-wins: a `Default` slot defers to the other side,
        // anything else (NoCache, Ttl) is treated as an explicit choice.
        if matches!(self.cache_policy, CachePolicy::Default) {
            self.cache_policy = other.cache_policy;
        }
    }
}

/// A named virtual filesystem node. Wraps [`Node`] with a name for use in
/// [`NodeAccumulator`]. Access capabilities via [`Deref`] to `Node`.
#[derive(Clone)]
pub struct NamedNode {
    name: String,
    node: Node,
}

impl NamedNode {
    pub fn new(name: impl Into<String>, node: Node) -> Self {
        Self {
            name: name.into(),
            node,
        }
    }

    /// Shorthand for a bare named file node (no capabilities).
    pub fn file(name: impl Into<String>) -> Self { Node::file().named(name) }

    /// Shorthand for a bare named directory node (no capabilities).
    pub fn dir(name: impl Into<String>) -> Self { Node::dir().named(name) }

    pub fn name(&self) -> &str { &self.name }

    pub fn set_name(&mut self, name: impl Into<String>) { self.name = name.into(); }

    /// Merge capabilities from another named node into this one.
    /// Names must match — the incoming name is discarded.
    pub fn merge_capabilities_from(&mut self, other: Self) {
        debug_assert_eq!(self.name, other.name, "cannot merge nodes with different names");
        self.node.merge_capabilities_from(other.node);
    }

    /// Consume into parts.
    pub fn into_parts(self) -> (String, Node) { (self.name, self.node) }
}

impl Deref for NamedNode {
    type Target = Node;

    fn deref(&self) -> &Node { &self.node }
}

impl DerefMut for NamedNode {
    fn deref_mut(&mut self) -> &mut Node { &mut self.node }
}

impl fmt::Debug for NamedNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NamedNode")
            .field("name", &self.name)
            .field("node", &self.node)
            .finish()
    }
}

/// Accumulates named nodes from multiple providers, merging capabilities for
/// same-name entries.
pub struct NodeAccumulator {
    nodes: Vec<NamedNode>,
}

impl NodeAccumulator {
    pub const fn new() -> Self { Self { nodes: Vec::new() } }

    /// Add a node. If a node with the same name already exists, merge
    /// capabilities (first-writer-wins for contested slots).
    pub fn add(&mut self, node: NamedNode) {
        if let Some(existing) = self.nodes.iter_mut().find(|n| n.name == node.name) {
            existing.merge_capabilities_from(node);
        } else {
            self.nodes.push(node);
        }
    }

    /// Add all nodes from an iterator, merging capabilities as with [`add`](Self::add).
    pub fn extend(&mut self, nodes: impl IntoIterator<Item = NamedNode>) {
        for node in nodes {
            self.add(node);
        }
    }

    /// Find a node by name (immutable).
    pub fn find(&self, name: &str) -> Option<&NamedNode> { self.nodes.iter().find(|n| n.name == name) }

    /// Find a node by name (mutable — for decoration).
    pub fn find_mut(&mut self, name: &str) -> Option<&mut NamedNode> { self.nodes.iter_mut().find(|n| n.name == name) }

    /// Iterate over all nodes.
    pub fn iter(&self) -> impl Iterator<Item = &NamedNode> { self.nodes.iter() }

    /// Iterate over all nodes mutably.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut NamedNode> { self.nodes.iter_mut() }

    /// Take all accumulated nodes.
    pub fn drain(&mut self) -> Vec<NamedNode> { mem::take(&mut self.nodes) }

    /// Remove nodes that don't match the predicate.
    pub fn retain(&mut self, f: impl FnMut(&NamedNode) -> bool) { self.nodes.retain(f); }

    /// Number of accumulated nodes.
    pub const fn len(&self) -> usize { self.nodes.len() }

    /// Whether there are no accumulated nodes.
    pub const fn is_empty(&self) -> bool { self.nodes.is_empty() }
}

impl Default for NodeAccumulator {
    fn default() -> Self { Self::new() }
}
#[cfg(test)]
mod tests;
