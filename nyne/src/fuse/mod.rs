//! FUSE filesystem handler.
//!
//! [`FuseFilesystem`] implements both [`crate::router::Filesystem`] (chain dispatch)
//! and [`fuser::Filesystem`] (FUSE protocol). It is the single entry point for
//! all filesystem operations — providers contribute content through the middleware
//! chain, and this module translates that into FUSE protocol responses.

/// FUSE file attribute helpers.
mod attrs;
/// File handle table for open files.
mod handles;
/// Bidirectional inode number mapping.
pub mod inode_map;
/// Capability → FUSE mode bit translation.
mod mode;
/// FUSE mutation operations (create, rename, unlink, mkdir, rmdir).
mod mutations;
/// Kernel cache invalidation notifications.
pub mod notify;
/// FUSE read-only operations (lookup, getattr, readdir, read, open).
mod ops;
/// Extended attribute handling for FUSE nodes.
mod xattr;

use std::collections::HashMap;
use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, OnceLock, PoisonError, RwLock};
use std::time::{Instant, SystemTime};

use color_eyre::eyre::{Report, Result};
use fuser::Errno;
use handles::HandleTable;
use inode_map::InodeMap;
use notify::KernelNotifier;

use crate::err::io_err;
use crate::path_utils::PathExt;
use crate::procfs::read_comm;
use crate::router::{
    AffectedFiles, Chain, DirEntry, Filesystem, Metadata, NamedNode, Op, Process, ReadContext, RenameContext, Request,
    UnlinkContext, WriteContext,
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
    backing_fs: Arc<dyn Filesystem>,
    /// Bidirectional inode ↔ path mapping.
    inodes: Arc<InodeMap>,
    /// File handle table for buffered/direct I/O.
    handles: HandleTable,
    /// Per-inode write locks — prevents concurrent write pipelines.
    write_locks: RwLock<HashMap<u64, Arc<Mutex<()>>>>,
    /// Per-inode last write error messages (exposed via `user.error` xattr).
    write_errors: RwLock<HashMap<u64, String>>,
    /// Whether FUSE kernel passthrough is available.
    passthrough_enabled: AtomicBool,
    /// Per-inode atime overrides for real files.
    atime_overrides: RwLock<HashMap<u64, SystemTime>>,
    /// Kernel cache invalidation notifier (set after FUSE mount).
    notifier: SharedNotifier,
    /// Paths recently written via the inline FUSE mutation path, used by
    /// the filesystem watcher to suppress its own fsnotify echoes of
    /// inline writes. See [`InlineWrites`] for the full rationale.
    inline_writes: InlineWrites,
    /// Cached PID → process name. Avoids repeated `/proc/{pid}/comm` reads
    /// on every FUSE request — a single PID may generate thousands of ops.
    process_names: RwLock<HashMap<u32, Option<String>>>,
}

impl FuseFilesystem {
    pub fn new(chain: Arc<Chain>, backing_fs: Arc<dyn Filesystem>) -> Self {
        Self {
            chain,
            backing_fs,
            inodes: Arc::new(InodeMap::new()),
            handles: HandleTable::new(),
            write_locks: RwLock::new(HashMap::new()),
            write_errors: RwLock::new(HashMap::new()),
            passthrough_enabled: AtomicBool::new(true),
            atime_overrides: RwLock::new(HashMap::new()),
            notifier: Arc::new(OnceLock::new()),
            inline_writes: Arc::new(Mutex::new(HashMap::new())),
            process_names: RwLock::new(HashMap::new()),
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
    /// hit procfs once.
    pub(super) fn process_from(&self, fuse_req: &fuser::Request) -> Process {
        let pid = fuse_req.pid();
        if let Some(name) = self
            .process_names
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&pid)
        {
            return Process {
                pid,
                name: name.clone(),
            };
        }
        let name = read_comm(pid);
        let mut cache = self.process_names.write().unwrap_or_else(PoisonError::into_inner);
        // Evict stale entries when the cache grows too large. PIDs are
        // recycled by the OS, so old entries for exited processes accumulate.
        if cache.len() >= 4096 {
            cache.clear();
        }
        cache.insert(pid, name.clone());
        Process { pid, name }
    }

    /// Dispatch a readdir through the chain, returning full `NamedNodes`.
    pub(super) fn read_dir_nodes(&self, path: &Path, process: Option<Process>) -> Result<Vec<NamedNode>> {
        let mut req = Request::new(path.to_path_buf(), Op::Readdir).with_opt_process(process);
        self.chain.dispatch(&mut req)?;
        Ok(req.nodes.drain())
    }

    /// Dispatch a lookup through the chain, returning the full `NamedNode`.
    pub(super) fn lookup_node(&self, dir: &Path, name: &str, process: Option<Process>) -> Result<Option<NamedNode>> {
        let mut req = Request::new(dir.to_path_buf(), Op::Lookup { name: name.to_owned() }).with_opt_process(process);
        self.chain.dispatch(&mut req)?;
        Ok(req.nodes.drain().into_iter().next())
    }

    /// Load file content for an inode via the [`Filesystem`] trait.
    pub(super) fn load_content(&self, ino: u64) -> Result<Arc<[u8]>> {
        Ok(Arc::from(Filesystem::read_file(self, &self.inode_path(ino)?)?))
    }

    /// Flush written content for an inode via the [`Filesystem`] trait.
    pub(super) fn flush_content(&self, ino: u64, data: &[u8]) -> Result<()> {
        Filesystem::write_file(self, &self.inode_path(ino)?, data)?;
        Ok(())
    }

    /// Resolve an inode to its full path, or error if unknown.
    fn inode_path(&self, ino: u64) -> Result<PathBuf> { self.inodes.resolve_path(ino) }


    /// Dispatch a single-path mutation (create, remove, mkdir) through the chain.
    fn dispatch_path_op(&self, path: &Path, op_fn: impl FnOnce(String) -> Op, process: Option<Process>) -> Result<()> {
        let (dir, name) = split_path(path)?;
        self.chain
            .dispatch(&mut Request::new(dir.to_path_buf(), op_fn(name.to_owned())).with_opt_process(process))
    }


    /// Dispatch a rename operation through the chain.
    fn dispatch_rename_op(&self, from: &Path, to: &Path, process: Option<Process>) -> Result<()> {
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
    /// Resolve `path` to a [`NamedNode`], mapping missing entries to
    /// [`ErrorKind::NotFound`] so the FUSE layer can surface `ENOENT`.
    fn resolve_named(&self, path: &Path) -> Result<NamedNode> {
        let (dir, name) = split_path(path)?;
        self.lookup_node(dir, name, None)?
            .ok_or_else(|| io_err(ErrorKind::NotFound, format!("not found: {}", path.display())))
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
    pub(super) fn resolve_node_for_inode(&self, ino: u64) -> Result<Option<NamedNode>> {
        let Some(entry) = self.inodes.get(ino) else {
            return Ok(None);
        };
        self.lookup_node(&entry.dir_path, &entry.name, None)
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
    fn notify_change(&self, affected: &AffectedFiles) {
        if affected.is_empty() {
            return;
        }
        let now = Instant::now();
        let mut writes = self.inline_writes.lock().unwrap_or_else(PoisonError::into_inner);
        for path in affected {
            writes.insert(path.clone(), now);
        }
        drop(writes);

        if let Some(notifier) = self.notifier.get() {
            notify::propagate_source_changes(affected, &self.chain, notifier.as_ref(), &self.inodes);
        }
    }
}

impl Filesystem for FuseFilesystem {
    fn source_dir(&self) -> &Path { self.backing_fs.source_dir() }

    fn metadata(&self, path: &Path) -> Result<Metadata> { self.backing_fs.metadata(path) }

    fn symlink_target(&self, path: &Path) -> Result<PathBuf> { self.backing_fs.symlink_target(path) }

    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        Ok(self
            .read_dir_nodes(path, None)?
            .into_iter()
            .map(|n| DirEntry {
                name: n.name().to_owned(),
                kind: n.kind(),
            })
            .collect())
    }

    fn stat(&self, dir: &Path, name: &str) -> Result<Option<DirEntry>> {
        Ok(self.lookup_node(dir, name, None)?.map(|n| DirEntry {
            name: n.name().to_owned(),
            kind: n.kind(),
        }))
    }

    fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        let node = self.resolve_named(path)?;
        node.readable()
            .ok_or_else(|| io_err(ErrorKind::PermissionDenied, format!("not readable: {}", path.display())))?
            .read(&ReadContext {
                path,
                fs: self.backing_fs.as_ref(),
            })
    }


    fn write_file(&self, path: &Path, content: &[u8]) -> Result<AffectedFiles> {
        let affected = self
            .resolve_named(path)?
            .writable()
            .ok_or_else(|| io_err(ErrorKind::PermissionDenied, format!("not writable: {}", path.display())))?
            .write(
                &WriteContext {
                    path,
                    fs: self.backing_fs.as_ref(),
                },
                content,
            )?;

        self.notify_change(&affected);
        Ok(affected)
    }


    fn rename(&self, from: &Path, to: &Path) -> Result<()> { self.dispatch_rename_op(from, to, None) }

    fn remove(&self, path: &Path) -> Result<()> { self.dispatch_path_op(path, |name| Op::Remove { name }, None) }

    fn create_file(&self, path: &Path) -> Result<()> { self.dispatch_path_op(path, |name| Op::Create { name }, None) }

    fn mkdir(&self, path: &Path) -> Result<()> { self.dispatch_path_op(path, |name| Op::Mkdir { name }, None) }
}

/// Evaluate a fallible expression; on error, log + reply with the mapped errno.
macro_rules! fuse_try {
    ($reply:expr, $expr:expr, $ino:expr, $msg:literal) => {
        match $expr {
            Ok(val) => val,
            Err(e) => {
                let errno = extract_errno(&e);
                debug!(target: "nyne::fuse", ino = $ino, error = %e, errno = ?errno, $msg);
                $reply.error(errno);
                return;
            }
        }
    };
}

/// Resolve a parent inode to its directory path, or reply ENOENT.
macro_rules! ensure_dir_path {
    ($self:expr, $parent:expr, $reply:expr) => {
        match $self.inodes.dir_path_for($parent) {
            Some(path) => path,
            None => {
                $reply.error(Errno::ENOENT);
                return;
            }
        }
    };
}

pub(super) use ensure_dir_path;
pub(super) use fuse_try;

/// Split a path into (parent_dir, file_name), mapping a malformed path to
/// [`ErrorKind::InvalidInput`] so callers surface `EINVAL` rather than `EIO`.
fn split_path(path: &Path) -> Result<(&Path, &str)> {
    path.split_dir_name()
        .ok_or_else(|| io_err(ErrorKind::InvalidInput, format!("invalid path: {}", path.display())))
}

/// Extract a FUSE errno from an opaque eyre error chain.
///
/// Walks the error chain looking for [`std::io::Error`]. If found, uses
/// its raw OS errno (for real I/O errors) or maps its [`ErrorKind`] (for
/// synthetic errors). Falls back to [`Errno::EIO`] if no `io::Error` is
/// in the chain.
///
/// This is the SSOT for converting **dispatch-level errors** (provider
/// results, chain errors) into FUSE errnos. It exists because those
/// errors are opaque `Report`s whose errno must be discovered by walking
/// the cause chain.
///
/// Direct `Errno::` constants elsewhere in the FUSE layer (e.g., returning
/// `ENOENT` when an inode is not in the map, or `ENOTSUP` for an
/// unsupported xattr operation) are **not** violations of this SSOT —
/// those are deterministic validation checks where the errno is known
/// statically at the call site and no error chain exists to inspect.
fn extract_errno(e: &Report) -> Errno {
    for cause in e.chain() {
        if let Some(io_err) = cause.downcast_ref::<Error>() {
            if let Some(raw) = io_err.raw_os_error() {
                return Errno::from_i32(raw);
            }
            return match io_err.kind() {
                ErrorKind::NotFound => Errno::ENOENT,
                ErrorKind::AlreadyExists => Errno::from_i32(libc::EEXIST),
                ErrorKind::PermissionDenied => Errno::EACCES,
                ErrorKind::InvalidInput => Errno::EINVAL,
                ErrorKind::Unsupported => Errno::from_i32(libc::ENOTSUP),
                _ => Errno::EIO,
            };
        }
    }
    Errno::EIO
}
