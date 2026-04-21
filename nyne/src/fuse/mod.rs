//! FUSE filesystem handler.
//!
//! [`FuseFilesystem`] implements both [`crate::router::Filesystem`] (chain dispatch)
//! and [`fuser::Filesystem`] (FUSE protocol). It is the single entry point for
//! all filesystem operations — providers contribute content through the middleware
//! chain, and this module translates that into FUSE protocol responses.

/// FUSE file attribute helpers.
mod attrs;
/// `router::Filesystem` bridge implementation (chain-dispatch side).
mod filesystem_impl;
/// File handle table for open files.
mod handles;
/// Bidirectional inode number mapping.
pub mod inode_map;
/// Per-inode mutable state (write locks, errors, atime overrides).
mod inode_state;
/// FUSE protocol macros (fuse_try!, fuse_err!, ensure_dir_path!, prepare_mutation!, reply_enotsup!).
mod macros;
/// Capability → FUSE mode bit translation.
mod mode;
/// FUSE mutation operations (create, rename, unlink, mkdir, rmdir).
mod mutations;
/// Kernel cache invalidation notifications.
pub mod notify;
/// FUSE read-only operations (lookup, getattr, readdir, read, open).
mod ops;
/// Bounded PID → process-name cache.
mod process_name_cache;
/// Extended attribute handling for FUSE nodes.
mod xattr;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use color_eyre::eyre::Result;
use handles::HandleTable;
use inode_map::InodeMap;
use inode_state::InodeState;
pub(super) use macros::{ensure_dir_path, fuse_err, fuse_try, prepare_mutation, reply_enotsup};
use notify::KernelNotifier;
use parking_lot::Mutex;
use process_name_cache::ProcessNameCache;

use crate::err;
use crate::path_utils::PathExt;
use crate::router::{
    AffectedFiles, Chain, Filesystem, NamedNode, Op, Process, RenameContext, Request, UnlinkContext, WriteContext,
};

/// Shared notifier slot — populated after FUSE mount, read by the watcher.
pub type SharedNotifier = Arc<OnceLock<Box<dyn KernelNotifier + Send + Sync>>>;
/// Paths recently written via the inline FUSE mutation path, keyed by
/// their relative-to-source-root form and stamped with the insertion
/// time. Shared between [`FuseFilesystem::notify_change`] (producer) and
/// the filesystem watcher (consumer).
///
/// Every FUSE write that lands on a backing file produces two parallel
/// invalidation signals:
///
/// 1. [`FuseFilesystem::notify_change`] runs synchronously as part of
///    the write pipeline and propagates changes to providers and the
///    kernel cache before the write returns to userspace.
/// 2. The host kernel observes the same write via fsnotify and delivers
///    it to the filesystem watcher a few milliseconds later.
///
/// The watcher's pass is pure duplicate work for inline-originated
/// writes — and worse, the delayed second kernel cache invalidation
/// consistently races rustc's incremental fingerprint pass across cargo
/// invocations, corrupting the incremental compiler cache. This set
/// lets the watcher recognise its own echoes and drop them. External
/// writes (editors, git, etc.) bypass the inline path, never land in
/// this set, and still flow through the watcher normally.
pub type InlineWrites = Arc<Mutex<HashMap<PathBuf, Instant>>>;

/// The FUSE filesystem handler.
///
/// Implements [`crate::router::Filesystem`] (chain dispatch) and [`fuser::Filesystem`]
/// (FUSE protocol). Shared state (`chain`, `inodes`, `notifier`, `inline_writes`)
/// is `Arc`'d so the watcher and control server can access them after
/// `fuser::spawn_mount2` takes ownership.
pub struct FuseFilesystem {
    /// Middleware chain dispatching to providers.
    chain: Arc<Chain>,
    /// Backend filesystem for provider I/O (passed to ReadContext/WriteContext)
    /// and passthrough fd resolution (via `source_dir()` + `backing_path()`).
    pub(super) backing_fs: Arc<dyn Filesystem>,
    /// Bidirectional inode ↔ path mapping.
    inodes: Arc<InodeMap>,
    /// File handle table for buffered/direct I/O.
    handles: HandleTable,
    /// Per-inode mutable state (write locks, write errors, atime overrides).
    pub(super) inode_state: InodeState,
    /// Whether FUSE kernel passthrough is available.
    passthrough_enabled: AtomicBool,
    /// Kernel cache invalidation notifier (set after FUSE mount).
    notifier: SharedNotifier,
    /// Paths recently written via the inline FUSE mutation path, used by
    /// the filesystem watcher to suppress its own fsnotify echoes of
    /// inline writes. See [`InlineWrites`] for the full rationale.
    inline_writes: InlineWrites,
    /// Bounded PID → comm cache. See [`ProcessNameCache`].
    process_names: ProcessNameCache,
}

impl FuseFilesystem {
    pub fn new(chain: Arc<Chain>, backing_fs: Arc<dyn Filesystem>) -> Self {
        Self {
            chain,
            backing_fs,
            inodes: Arc::new(InodeMap::new()),
            handles: HandleTable::new(),
            inode_state: InodeState::default(),
            passthrough_enabled: AtomicBool::new(true),
            notifier: Arc::new(OnceLock::new()),
            inline_writes: Arc::new(Mutex::new(HashMap::new())),
            process_names: ProcessNameCache::new(),
        }
    }

    /// Access the inode map.
    pub(crate) const fn inodes(&self) -> &Arc<InodeMap> { &self.inodes }

    /// Access the kernel notifier slot.
    pub fn notifier(&self) -> &SharedNotifier { &self.notifier }

    /// Access the inline-write suppression set shared with the watcher.
    pub(crate) const fn inline_writes(&self) -> &InlineWrites { &self.inline_writes }

    /// Build a [`Process`](Process) from a FUSE request's PID.
    ///
    /// Caches the `/proc/{pid}/comm` read so repeated FUSE requests from
    /// the same PID (e.g., git status issuing thousands of lookups) only
    /// hit procfs once. See [`ProcessNameCache`] for eviction policy.
    pub(super) fn process_from(&self, fuse_req: &fuser::Request) -> Process {
        let pid = fuse_req.pid();
        Process {
            pid,
            name: self.process_names.get_or_read(pid),
        }
    }

    /// Dispatch a readdir through the chain, returning full `NamedNodes`.
    pub(super) fn read_dir_nodes(&self, path: &Path, process: Option<Process>) -> Result<Vec<NamedNode>> {
        let mut req = Request::new(path.to_path_buf(), Op::Readdir).with_opt_process(process);
        self.chain.dispatch(&mut req)?;
        Ok(req.nodes.drain())
    }

    /// Dispatch a lookup through the chain, returning the full `NamedNode`.
    ///
    /// Bound-node fast path: any inode that was given a node at allocation
    /// time (via [`InodeMap::bind_node`] — currently only `on_create`
    /// sinks like batch-edit `edit/{op}`) keeps that node addressable
    /// for the binding's TTL, after which the binding lazily clears and
    /// the chain dispatch decides. This makes `on_create`-materialized
    /// sinks survive `notify_change` cascades long enough for the post-
    /// write `statx` (and subsequent writes within the TTL window).
    pub(super) fn lookup_node(&self, dir: &Path, name: &str, process: Option<Process>) -> Result<Option<NamedNode>> {
        if let Some(ino) = self.inodes.find_inode(dir, name)
            && let Some(node) = self.inodes.bound_node(ino)
        {
            return Ok(Some(node));
        }
        let mut req = Request::new(dir.to_path_buf(), Op::Lookup { name: name.to_owned() }).with_opt_process(process);
        self.chain.dispatch(&mut req)?;
        Ok(req.nodes.drain().into_iter().next())
    }

    /// Load file content for an inode via the [`Filesystem`] trait.
    pub(super) fn load_content(&self, ino: u64) -> Result<Arc<[u8]>> {
        Ok(Arc::from(Filesystem::read_file(self, &self.inode_path(ino)?)?))
    }

    /// Flush written content for an inode.
    ///
    /// Resolves the target node via [`Filesystem::write_file`] → the
    /// usual `lookup_node` path. Bound `on_create` nodes are surfaced by
    /// `lookup_node`'s bound-node fast path, so a single codepath covers
    /// both regular and `on_create`-materialized writables.
    pub(super) fn flush_content(&self, ino: u64, data: &[u8]) -> Result<()> {
        Filesystem::write_file(self, &self.inode_path(ino)?, data)?;
        Ok(())
    }

    /// Invoke a node's [`Writable`] capability and notify change propagation.
    ///
    /// Single source of truth for the "extract writable → write → notify"
    /// sequence — consumed by [`Filesystem::write_file`] for both
    /// chain-resolved nodes and bound `on_create` nodes (both reach the
    /// node via `lookup_node`'s bound-node fast path).
    pub(super) fn write_via_node(&self, path: &Path, node: &NamedNode, data: &[u8]) -> Result<AffectedFiles> {
        let affected = node.writable().ok_or_else(|| err::not_writable(path))?.write(
            &WriteContext {
                path,
                fs: self.backing_fs.as_ref(),
            },
            data,
        )?;
        self.notify_change(&affected);
        Ok(affected)
    }

    /// Resolve an inode to its full path, or error if unknown.
    fn inode_path(&self, ino: u64) -> Result<PathBuf> { self.inodes.resolve_path(ino) }

    /// Dispatch a single-path mutation (create, remove, mkdir) through the chain.
    ///
    /// Returns the dispatched [`Request`] so callers can inspect nodes
    /// produced by op-specific callbacks (e.g. `on_create` attaching a
    /// node to bind to the new inode) or state set by middleware.
    pub(super) fn dispatch_path_op(
        &self,
        path: &Path,
        op_fn: impl FnOnce(String) -> Op,
        process: Option<Process>,
    ) -> Result<Request> {
        let (dir, name) = split_path(path)?;
        let mut req = Request::new(dir.to_path_buf(), op_fn(name.to_owned())).with_opt_process(process);
        self.chain.dispatch(&mut req)?;
        Ok(req)
    }

    /// Dispatch a rename operation through the chain.
    pub(super) fn dispatch_rename_op(&self, from: &Path, to: &Path, process: Option<Process>) -> Result<()> {
        let (src_dir, src_name) = split_path(from)?;
        let (dst_dir, dst_name) = split_path(to)?;
        self.chain.dispatch(
            &mut Request::new(src_dir.to_path_buf(), Op::Rename {
                src_name: src_name.to_owned(),
                target_dir: dst_dir.to_path_buf(),
                target_name: dst_name.to_owned(),
            })
            .with_opt_process(process),
        )
    }

    /// Try to dispatch a mutation via a node capability.
    ///
    /// Looks up the node at `path`, passes it to `invoke`. If the closure
    /// returns `Some(Ok(affected))`, notifies providers and returns `Ok(true)`.
    /// `None` means the node lacks the capability — returns `Ok(false)`.
    fn try_node_mutation(
        &self,
        path: &Path,
        invoke: impl FnOnce(&NamedNode) -> Option<Result<AffectedFiles>>,
    ) -> Result<bool> {
        let (dir, name) = split_path(path)?;
        let Some(node) = self.lookup_node(dir, name, None)? else {
            return Ok(false);
        };
        let Some(result) = invoke(&node) else {
            return Ok(false);
        };
        self.notify_change(&result?);
        Ok(true)
    }

    /// Try a node capability; on `Ok(false)`, fall back to a chain dispatch.
    ///
    /// Encapsulates the "VFS node capability first, real-FS chain second"
    /// pattern shared by [`Self::do_remove`](super::mutations) and
    /// [`Self::do_rename`](super::mutations).
    fn try_node_then_chain(
        node_attempt: impl FnOnce() -> Result<bool>,
        chain_fallback: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        if node_attempt()? { Ok(()) } else { chain_fallback() }
    }

    /// Writable-check + chain-dispatch + resolve the freshly created entry.
    ///
    /// Shared prelude for [`Self::do_create`](super::mutations) and
    /// [`Self::do_mkdir`](super::mutations) — both reject in non-writable
    /// parents (`EACCES`), dispatch the op through the chain, then resolve
    /// the new entry to `(inode, node)` for the FUSE reply.
    ///
    /// If a router `on_create` callback attached a node to `req.nodes` for
    /// the to-be-created name (e.g. batch-edit `edit/{op}` sinks), the
    /// node is bound to the inode via [`InodeMap::bind_node`] with the
    /// TTL declared by its [`CachePolicy`]. Subsequent `lookup_node`
    /// calls return that bound node directly until the TTL elapses,
    /// after which the binding lazily clears and lookups fall through
    /// to the chain (which doesn't surface the sink) → `ENOENT`.
    /// Provider-backed real-file creates (e.g. the `fs` plugin) attach
    /// no node and fall through to [`resolve_inode`](Self::resolve_inode).
    pub(super) fn dispatch_and_resolve_path_op(
        &self,
        req: &fuser::Request,
        parent: u64,
        dir_path: &Path,
        name: &str,
        op_fn: impl FnOnce(String) -> Op,
    ) -> Result<Option<(u64, NamedNode)>> {
        let path = dir_path.join(name);
        if !self.is_writable_dir(parent, req) {
            return Err(err::not_writable(&path));
        }
        let process = self.process_from(req);
        let dispatched = self.dispatch_path_op(&path, op_fn, Some(process.clone()))?;

        if let Some(node) = dispatched.nodes.find(name).cloned() {
            let ino = self.ensure_inode(dir_path, name, parent);
            self.inodes.bind_node(ino, node.clone());
            return Ok(Some((ino, node)));
        }

        self.resolve_inode(dir_path, name, parent, Some(process))
    }

    /// Resolve `path` to a [`NamedNode`], mapping missing entries to
    /// [`ErrorKind::NotFound`] so the FUSE layer can surface `ENOENT`.
    pub(super) fn resolve_named(&self, path: &Path) -> Result<NamedNode> {
        let (dir, name) = split_path(path)?;
        self.lookup_node(dir, name, None)?.ok_or_else(|| err::not_found(path))
    }

    /// Try to rename via the node's [`Renameable`] capability.
    pub(super) fn rename_node(&self, from: &Path, to: &Path) -> Result<bool> {
        self.try_node_mutation(from, |node| {
            Some(node.renameable()?.rename(&RenameContext {
                source: from,
                target: to,
            }))
        })
    }

    /// Try to remove via the node's [`Unlinkable`] capability.
    pub(super) fn remove_node(&self, path: &Path) -> Result<bool> {
        self.try_node_mutation(path, |node| Some(node.unlinkable()?.unlink(&UnlinkContext { path })))
    }

    /// Resolve an inode to its `NamedNode` via the inode map + chain dispatch.
    ///
    /// Combines the inode map lookup and chain dispatch into a single call.
    /// Returns `Ok(None)` if the inode is unknown or the node no longer exists.
    ///
    /// `lookup_node` consults [`InodeMap::bound_node`] first, so inodes
    /// allocated with a bound node (e.g. `on_create` sinks) resolve
    /// without round-tripping through the chain.
    pub(super) fn resolve_node_for_inode(&self, ino: u64) -> Result<Option<NamedNode>> {
        let Some(entry) = self.inodes.get(ino) else {
            return Ok(None);
        };
        self.lookup_node(&entry.dir_path, &entry.name, None)
    }

    /// Lifecycle + bound-node TTL bookkeeping for a freshly opened handle.
    ///
    /// Single SSOT for the open-side `Lifecycle::on_open` + `InodeMap::touch`
    /// pair. Called from both [`super::ops::FuseFilesystem::open`] (regular
    /// FUSE OPEN) and [`super::mutations::FuseFilesystem::do_create`]
    /// (CREATE atomically opens, bypassing the OPEN op).
    pub(super) fn notify_open(&self, ino: u64, node: &NamedNode) {
        if let Some(lc) = node.lifecycle() {
            lc.on_open();
        }
        self.inodes.touch(ino);
    }

    /// Lifecycle + bound-node TTL bookkeeping for a fully released handle.
    ///
    /// Mirrors [`Self::notify_open`] for the close side: invokes the
    /// node's `Lifecycle::on_close` and refreshes the bound-node TTL so
    /// the binding outlives the post-release `statx` window. No-op for
    /// nodes without a `Lifecycle` and inodes without a bound TTL.
    pub(super) fn notify_close(&self, ino: u64, node: &NamedNode) {
        if let Some(lc) = node.lifecycle() {
            lc.on_close();
        }
        self.inodes.touch(ino);
    }

    /// Notify providers of changed source files and invalidate kernel caches.
    ///
    /// Called synchronously after VFS writes and node mutations so caches
    /// invalidate before the next read — the async watcher has a 50ms
    /// debounce that would leave a stale window.
    ///
    /// Also records each affected path in [`inline_writes`](Self::inline_writes)
    /// so the filesystem watcher can recognise its own fsnotify echoes of
    /// this write and drop them instead of re-invalidating the kernel
    /// cache a few milliseconds later (which would race rustc's
    /// incremental fingerprint pass and corrupt its cache).
    ///
    /// Delegates to [`notify::propagate_source_changes`] — the single source
    /// of truth for change propagation, shared with the filesystem watcher.
    pub(super) fn notify_change(&self, affected: &AffectedFiles) {
        if affected.is_empty() {
            return;
        }
        let now = Instant::now();
        let mut writes = self.inline_writes.lock();
        for path in affected {
            writes.insert(path.clone(), now);
        }
        drop(writes);

        if let Some(notifier) = self.notifier.get() {
            notify::propagate_source_changes(affected, &self.chain, notifier.as_ref(), &self.inodes);
        }
    }
}

/// Split a path into (`parent_dir`, `file_name`), mapping a malformed path to
/// [`ErrorKind::InvalidInput`] so callers surface `EINVAL` rather than `EIO`.
fn split_path(path: &Path) -> Result<(&Path, &str)> { path.split_dir_name().ok_or_else(|| err::invalid_path(path)) }
