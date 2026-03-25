//! FUSE filesystem handler — maps fuser callbacks to router operations.

/// FUSE file attribute helpers.
mod attrs;
/// File handle table for open files.
mod handles;
/// Kernel cache invalidation notifications.
mod notify;
/// Per-process visibility filtering for VFS entries.
mod visibility_map;

use attrs::file_kind_to_fuse;
pub use notify::{AsyncNotifier, FuseNotifier};
pub use visibility_map::VisibilityMap;

// Re-export for sibling submodules (mutations.rs uses GENERATION).
use self::attrs::GENERATION;

/// Evaluate a fallible expression; on error, log + reply with the
/// extracted errno + return from the enclosing FUSE callback.
///
/// On `Ok(v)`, evaluates to `v`. The trailing arguments are passed
/// directly to `tracing::warn!` (structured fields + message).
macro_rules! fuse_try {
    ($reply:expr, $expr:expr, $($fields:tt)*) => {
        match $expr {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(target: "nyne::fuse", error = %e, $($fields)*);
                $reply.error(extract_errno(&e));
                return;
            }
        }
    };
}

/// Resolve a parent inode to its `VfsPath`, or reply `ENOENT` and
/// return from the enclosing FUSE callback.
macro_rules! ensure_dir_path {
    ($self:expr, $parent:expr, $reply:expr) => {
        match $self.router.dir_path_for_inode($parent) {
            Some(path) => path,
            None => {
                $reply.error(Errno::ENOENT);
                return;
            }
        }
    };
}

/// Shared preamble for FUSE callbacks that operate on a (parent, name)
/// pair: converts the parent `INodeNo` to `u64`, lossy-converts the
/// name, logs a debug line, resolves the parent directory path (replying
/// `ENOENT` on failure), and builds a `RequestContext`.
macro_rules! with_parent_ctx {
    ($self:expr, $parent:expr, $name:expr, $reply:expr, $label:literal, |$ino:ident, $name_str:ident, $ctx:ident| $body:expr) => {{
        let $ino = u64::from($parent);
        let $name_str = $name.to_string_lossy();
        let dir_path = ensure_dir_path!($self, $ino, $reply);
        debug!(target: "nyne::fuse", $ino, path = %dir_path, name = %$name_str, $label);
        let $ctx = $self.router.make_request_context(&dir_path);
        $body
    }};
}

/// FUSE mutation operations (write, create, rename, unlink, mkdir, rmdir).
mod mutations;
/// FUSE read-only operations (lookup, getattr, readdir, read, open).
mod ops;
/// Extended attribute handling for FUSE nodes.
mod xattr;

use std::collections::HashMap;
use std::io::{Error, ErrorKind};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

use color_eyre::eyre::{Report, Result, eyre};
use fuser::{Errno, Request};
use parking_lot::{Mutex, RwLock};
use tracing::debug;

use self::handles::HandleTable;
use crate::dispatch::context::RequestContext;
use crate::dispatch::{ReaddirEntry, ResolvedInode, Router, WriteMode};
use crate::node::VirtualNode;
use crate::provider::Provider;
use crate::types::ProcessVisibility;
use crate::types::file_kind::FileKind;
use crate::types::vfs_path::VfsPath;

/// The FUSE filesystem handler.
///
/// Per-inode state (`write_locks`, `write_errors`, `atime_overrides`) is
/// intentionally split across separate `RwLock<HashMap>`s rather than
/// consolidated behind a single lock. Each map has a distinct access pattern:
/// write locks are held for the duration of write pipelines, write errors are
/// set/cleared on flush, and atime overrides are rare hook-driven updates.
/// Separate locks avoid contention between these unrelated operations.
pub struct NyneFs {
    router: Arc<Router>,
    handles: HandleTable,
    /// Per-inode write locks.
    ///
    /// Prevents concurrent write pipelines for the same inode.
    /// The spec requires acquiring a lock before executing the write
    /// pipeline and releasing it after post-write hooks complete.
    write_locks: RwLock<HashMap<u64, Arc<Mutex<()>>>>,
    /// Per-inode last write error messages.
    ///
    /// Populated on flush failure, cleared on flush success. Exposed
    /// via the `user.error` extended attribute so that `PostToolUse`
    /// hooks can surface validation errors to the agent.
    write_errors: RwLock<HashMap<u64, String>>,
    /// Whether FUSE kernel passthrough is available.
    ///
    /// Set to `true` in `init()` if the kernel accepts `max_stack_depth=1`.
    /// Flipped to `false` on the first `open_backing` rejection — avoids
    /// repeated syscalls and log spam when passthrough isn't usable.
    passthrough_enabled: AtomicBool,
    /// Per-inode atime overrides for real files.
    ///
    /// Session-scoped, in-memory only. Used by hooks (e.g., raw file
    /// interception) to store timestamps via `touch -a`. Returns
    /// `UNIX_EPOCH` for inodes without an override.
    atime_overrides: RwLock<HashMap<u64, SystemTime>>,
    /// Per-process visibility resolution.
    ///
    /// Shared with the control server so `SetVisibility` requests take
    /// effect immediately. Determines whether each process sees all
    /// virtual nodes, only non-hidden ones, or only the real filesystem.
    visibility: Arc<VisibilityMap>,
}

/// Core FUSE handler methods for inode resolution and content I/O.
impl NyneFs {
    /// Creates a new FUSE filesystem handler.
    pub fn new(router: Arc<Router>, visibility: Arc<VisibilityMap>) -> Self {
        Self {
            router,
            handles: HandleTable::new(),
            write_locks: RwLock::new(HashMap::new()),
            write_errors: RwLock::new(HashMap::new()),
            passthrough_enabled: AtomicBool::new(false),
            atime_overrides: RwLock::new(HashMap::new()),
            visibility,
        }
    }

    /// Dispatches inode operations to real or virtual handlers.
    fn with_inode_io<T>(
        &self,
        ino: u64,
        on_real: impl FnOnce(&VfsPath) -> Result<T>,
        on_virtual: impl FnOnce(&VirtualNode, &dyn Provider, &RequestContext<'_>) -> Result<T>,
    ) -> Result<T> {
        // ROOT_INODE is a sentinel not stored in the inode map — handle
        // it as a real directory at VfsPath::root().
        if ino == Router::ROOT_INODE {
            return on_real(&VfsPath::root());
        }
        match self
            .router
            .resolve_inode(ino)
            .ok_or_else(|| eyre!("inode {ino} not found"))?
        {
            ResolvedInode::Real { path, .. } => on_real(&path),
            ResolvedInode::Virtual {
                node,
                provider_id,
                dir_path,
            } => {
                let ctx = self.router.make_request_context(&dir_path);
                let provider = self
                    .router
                    .find_provider(provider_id)
                    .ok_or_else(|| eyre!("provider {provider_id} not found"))?;
                on_virtual(&node, provider.as_ref(), &ctx)
            }
        }
    }

    /// Load file content for an inode.
    ///
    /// For virtual nodes, reads through the L2 content cache.
    /// For real files, reads directly from the filesystem (no caching).
    fn load_content(&self, ino: u64) -> Result<Arc<[u8]>> {
        self.with_inode_io(
            ino,
            |path| self.router.real_fs().read(path).map(Arc::from),
            |node, provider, ctx| self.router.read_content(ino, node, provider, ctx),
        )
    }

    /// Flush written content back for an inode.
    ///
    /// Acquires a per-inode write lock to prevent concurrent write
    /// pipelines racing on the same node. The lock is `Arc`-wrapped
    /// so the map lock is released before the write pipeline runs —
    /// without this, two unrelated inodes would serialize their
    /// entire write pipelines behind the map lock.
    fn flush_content(&self, ino: u64, data: &[u8], mode: WriteMode) -> Result<()> {
        let mutex = if let Some(m) = self.write_locks.read().get(&ino) {
            Arc::clone(m)
        } else {
            let mut locks = self.write_locks.write();
            Arc::clone(locks.entry(ino).or_insert_with(|| Arc::new(Mutex::new(()))))
        };
        let _guard = mutex.lock();

        self.with_inode_io(
            ino,
            |path| self.router.real_fs().write(path, data),
            |node, provider, ctx| {
                self.router.write_content(ino, node, provider, data, mode, ctx)?;
                Ok(())
            },
        )
    }

    /// Resolve the visibility level for the requesting process.
    fn process_visibility(&self, req: &Request) -> ProcessVisibility { self.visibility.resolve(req.pid()) }

    /// Resolve an inode, demoting virtual nodes to real for passthrough
    /// (`None` visibility) processes.
    ///
    /// This is the **single chokepoint** for per-process inode resolution.
    /// All FUSE handlers that operate on an existing inode (getattr, open,
    /// read, setattr, etc.) should call this instead of
    /// `router.resolve_inode` directly.
    ///
    /// Directory-enumeration handlers (lookup, readdir) control *name
    /// visibility* separately via `lookup_real` / `readdir_real`.
    fn resolve_for_request(&self, ino: u64, req: &Request) -> Option<ResolvedInode> {
        let resolved = self.router.resolve_inode(ino)?;
        if self.process_visibility(req) != ProcessVisibility::None {
            return Some(resolved);
        }
        // Passthrough: demote virtual nodes to real if the underlying file exists.
        match resolved {
            ResolvedInode::Real { .. } => Some(resolved),
            ResolvedInode::Virtual { node, dir_path, .. } => {
                let real_path = dir_path.join(node.name()).ok()?;
                self.router.real_fs().exists(&real_path).then(|| {
                    let file_type = self
                        .router
                        .real_fs()
                        .metadata(&real_path)
                        .map(|m| m.file_type)
                        .unwrap_or(FileKind::File);
                    ResolvedInode::Real {
                        file_type,
                        path: real_path,
                    }
                })
            }
        }
    }

    /// Shared iteration for `readdir` and `readdirplus`.
    ///
    /// Resolves passthrough inodes (inode 0) and calls `emit` for each
    /// entry. The `visibility` level controls which nodes appear (see
    /// [`Router::collect_readdir_entries`]).
    /// `emit` returns `true` when the reply buffer is full.
    fn for_each_readdir_entry(
        &self,
        dir_path: &VfsPath,
        dir_ino: u64,
        offset: u64,
        visibility: ProcessVisibility,
        emit: impl FnMut(u64, u64, &ReaddirEntry) -> bool,
    ) -> bool {
        let ctx = self.router.make_request_context(dir_path);
        let entries = self.router.collect_readdir_entries(dir_path, dir_ino, visibility);
        Self::iter_readdir_entries(&entries, offset, |name| self.router.lookup_name(name, &ctx), emit)
    }

    /// Shared iteration for readdir serving only real filesystem entries.
    ///
    /// Used by passthrough processes (git, LSP servers) that must never see
    /// virtual nodes. Reads real directory entries directly, bypassing all
    /// provider resolution.
    fn for_each_real_readdir_entry(
        &self,
        dir_path: &VfsPath,
        dir_ino: u64,
        offset: u64,
        emit: impl FnMut(u64, u64, &ReaddirEntry) -> bool,
    ) -> bool {
        let ctx = self.router.make_request_context(dir_path);
        let entries = self.router.readdir_real(dir_path, dir_ino);
        Self::iter_readdir_entries(&entries, offset, |name| self.router.lookup_real(name, &ctx), emit)
    }

    /// Resolve directory entries and emit them via the callback.
    ///
    /// Shared logic for `readdir` and `readdirplus` — resolves the dir path,
    /// checks visibility, ensures providers are resolved, then iterates
    /// entries through the caller-provided `emit` closure.
    fn resolve_readdir(
        &self,
        req: &Request,
        ino: u64,
        offset: u64,
        mut emit: impl FnMut(u64, u64, &ReaddirEntry) -> bool,
    ) -> Result<()> {
        let dir_path = self
            .router
            .dir_path_for_inode(ino)
            .ok_or_else(|| Error::from(ErrorKind::NotFound))?;
        let vis = self.process_visibility(req);
        debug!(target: "nyne::fuse", ino, offset, path = %dir_path, %vis, "readdir");

        if vis == ProcessVisibility::None {
            self.for_each_real_readdir_entry(&dir_path, ino, offset, &mut emit);
            return Ok(());
        }

        self.router
            .ensure_resolved(&self.router.make_request_context(&dir_path))?;
        self.for_each_readdir_entry(&dir_path, ino, offset, vis, emit);
        Ok(())
    }

    /// Core readdir iteration: resolve placeholder inodes and emit entries.
    ///
    /// `lookup` resolves inode-0 placeholders to real inodes. `emit` returns
    /// `true` when the reply buffer is full (stop iterating).
    fn iter_readdir_entries(
        entries: &[ReaddirEntry],
        offset: u64,
        mut lookup: impl FnMut(&str) -> Result<Option<u64>>,
        mut emit: impl FnMut(u64, u64, &ReaddirEntry) -> bool,
    ) -> bool {
        let skip = usize::try_from(offset).unwrap_or(0);
        for (i, entry) in entries.iter().enumerate().skip(skip) {
            // Passthrough entries use inode 0 as a placeholder — resolve
            // via lookup so we have a real inode for the attr response.
            let entry_inode = match entry.inode {
                0 => match lookup(&entry.name) {
                    Ok(Some(ino)) => ino,
                    _ => continue,
                },
                ino => ino,
            };
            let next_offset = u64::try_from(i + 1).unwrap_or(u64::MAX);
            if emit(entry_inode, next_offset, entry) {
                return true;
            }
        }
        false
    }
}

/// Extract a FUSE errno from an eyre error chain.
///
/// Walks the error chain looking for [`std::io::Error`]. If found, uses
/// its raw OS errno (for real I/O errors from `std::fs`) or maps its
/// [`ErrorKind`](std::io::ErrorKind) (for synthetic errors tagged by the
/// dispatch layer). Falls back to [`Errno::EIO`] if no `io::Error` is
/// in the chain.
///
/// This is the SSOT for error → errno conversion. All FUSE `Err` arms
/// should use this instead of hardcoding `Errno::EIO`.
fn extract_errno(e: &Report) -> Errno {
    for cause in e.chain() {
        if let Some(io_err) = cause.downcast_ref::<Error>() {
            // Real OS errors carry the exact errno.
            if let Some(raw) = io_err.raw_os_error() {
                return Errno::from_i32(raw);
            }
            // Synthetic errors (from dispatch layer) carry ErrorKind.
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
