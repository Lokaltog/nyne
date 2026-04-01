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
use std::time::SystemTime;

use color_eyre::eyre::{Report, Result, eyre};
use fuser::Errno;
use handles::HandleTable;
use inode_map::InodeMap;
use notify::KernelNotifier;

use crate::path_utils::PathExt;
use crate::procfs::read_comm;
use crate::router::{
    AffectedFiles, Chain, DirEntry, Filesystem, Metadata, NamedNode, Op, Process, ReadContext, RenameContext, Request,
    UnlinkContext, WriteContext,
};

/// Shared notifier slot — populated after FUSE mount, read by the watcher.
pub type SharedNotifier = Arc<OnceLock<Box<dyn KernelNotifier + Send + Sync>>>;

/// The FUSE filesystem handler.
///
/// Implements [`crate::router::Filesystem`] (chain dispatch) and [`fuser::Filesystem`]
/// (FUSE protocol). Shared state (`chain`, `inodes`, `notifier`) is `Arc`'d so
/// the watcher and control server can access them after `fuser::spawn_mount2`
/// takes ownership.
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
            process_names: RwLock::new(HashMap::new()),
        }
    }

    /// Access the inode map.
    pub(crate) const fn inodes(&self) -> &Arc<InodeMap> { &self.inodes }

    /// Access the kernel notifier slot.
    pub fn notifier(&self) -> &SharedNotifier { &self.notifier }

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
        self.process_names
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(pid, name.clone());
        Process { pid, name }
    }

    /// Dispatch a readdir through the chain, returning full `NamedNodes`.
    pub(super) fn read_dir_nodes(&self, path: &Path, process: Option<Process>) -> Result<Vec<NamedNode>> {
        let mut req = Request::new(path.to_path_buf(), Op::Readdir);
        if let Some(p) = process {
            req = req.with_process(p);
        }
        self.chain.dispatch(&mut req)?;
        Ok(req.nodes.drain())
    }

    /// Dispatch a lookup through the chain, returning the full `NamedNode`.
    pub(super) fn lookup_node(&self, dir: &Path, name: &str, process: Option<Process>) -> Result<Option<NamedNode>> {
        let mut req = Request::new(dir.to_path_buf(), Op::Lookup { name: name.to_owned() });
        if let Some(p) = process {
            req = req.with_process(p);
        }
        self.chain.dispatch(&mut req)?;
        Ok(req.nodes.drain().into_iter().next())
    }

    /// Load file content for an inode via the [`Filesystem`] trait.
    pub(super) fn load_content(&self, ino: u64) -> Result<Arc<[u8]>> {
        let path = self.inode_path(ino)?;
        Ok(Arc::from(Filesystem::read_file(self, &path)?))
    }

    /// Flush written content for an inode via the [`Filesystem`] trait.
    pub(super) fn flush_content(&self, ino: u64, data: &[u8]) -> Result<()> {
        Filesystem::write_file(self, &self.inode_path(ino)?, data)?;
        Ok(())
    }

    /// Resolve an inode to its full path, or error if unknown.
    fn inode_path(&self, ino: u64) -> Result<PathBuf> {
        self.inodes.full_path(ino).ok_or_else(|| eyre!("inode {ino} not found"))
    }

    /// Dispatch a single-path mutation (create, remove, mkdir) through the chain.
    fn dispatch_path_op(&self, path: &Path, op_fn: impl FnOnce(String) -> Op, process: Option<Process>) -> Result<()> {
        let (dir, name) = path
            .split_dir_name()
            .ok_or_else(|| eyre!("invalid path: {}", path.display()))?;
        let mut req = Request::new(dir.to_path_buf(), op_fn(name.to_owned()));
        if let Some(p) = process {
            req = req.with_process(p);
        }
        self.chain.dispatch(&mut req)
    }

    /// Dispatch a rename operation through the chain.
    fn dispatch_rename_op(&self, from: &Path, to: &Path, process: Option<Process>) -> Result<()> {
        let (src_dir, src_name) = from
            .split_dir_name()
            .ok_or_else(|| eyre!("invalid path: {}", from.display()))?;
        let (dst_dir, dst_name) = to
            .split_dir_name()
            .ok_or_else(|| eyre!("invalid path: {}", to.display()))?;
        let mut req = Request::new(src_dir.to_path_buf(), Op::Rename {
            src_name: src_name.to_owned(),
            target_dir: dst_dir.to_path_buf(),
            target_name: dst_name.to_owned(),
        });
        if let Some(p) = process {
            req = req.with_process(p);
        }
        self.chain.dispatch(&mut req)
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
        let (dir, name) = path
            .split_dir_name()
            .ok_or_else(|| eyre!("invalid path: {}", path.display()))?;
        let Some(node) = self.lookup_node(dir, name, None)? else {
            return Ok(false);
        };
        let Some(result) = invoke(&node) else {
            return Ok(false);
        };
        let affected = result?;
        for provider in self.chain.providers() {
            provider.on_change(&affected);
        }
        Ok(true)
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
        let (dir, name) = path
            .split_dir_name()
            .ok_or_else(|| eyre!("invalid path: {}", path.display()))?;
        let node = self
            .lookup_node(dir, name, None)?
            .ok_or_else(|| eyre!("not found: {}", path.display()))?;
        node.readable()
            .ok_or_else(|| eyre!("not readable: {}", path.display()))?
            .read(&ReadContext {
                path,
                fs: self.backing_fs.as_ref(),
            })
    }

    fn write_file(&self, path: &Path, content: &[u8]) -> Result<AffectedFiles> {
        let (dir, name) = path
            .split_dir_name()
            .ok_or_else(|| eyre!("invalid path: {}", path.display()))?;
        let node = self
            .lookup_node(dir, name, None)?
            .ok_or_else(|| eyre!("not found: {}", path.display()))?;
        let affected = node
            .writable()
            .ok_or_else(|| eyre!("not writable: {}", path.display()))?
            .write(
                &WriteContext {
                    path,
                    fs: self.backing_fs.as_ref(),
                },
                content,
            )?;

        // Notify providers synchronously so caches invalidate before
        // the next read — the async watcher has a 50ms debounce.
        if !affected.is_empty() {
            for provider in self.chain.providers() {
                provider.on_change(&affected);
            }

            // Invalidate the kernel page cache for each affected source file.
            // The splice wrote to the backing file via OsFilesystem, but the
            // kernel's FUSE page cache for those inodes is stale. Without this,
            // immediate reads of the source file return old cached content.
            if let Some(notifier) = self.notifier.get() {
                for source_path in &affected {
                    notify::invalidate_inode_at(source_path, notifier.as_ref(), &self.inodes);
                }
            }
        }
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
