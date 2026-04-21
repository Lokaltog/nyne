//! `fuser::Filesystem` trait implementation — all FUSE protocol callbacks.
//!
//! Each method dispatches through the middleware chain via [`FuseFilesystem`]
//! and replies with the appropriate FUSE response.
//!
//! Mutation callbacks (`create`, `mkdir`, `unlink`, `rmdir`, `rename`) delegate
//! to `mutations.rs`. Extended attribute callbacks delegate to `xattr.rs`.

use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime};

use color_eyre::eyre;
use fuser::{
    AccessFlags, BsdFileFlags, Errno, FileHandle, Filesystem, FopenFlags, INodeNo, InitFlags, LockOwner, OpenFlags,
    RenameFlags, ReplyAttr, ReplyCreate, ReplyData, ReplyEmpty, ReplyEntry, ReplyOpen, ReplyStatfs, ReplyWrite,
    ReplyXattr, Request, TimeOrNow, WriteFlags,
};
use rustix::fs::statvfs;
use tracing::{debug, info, trace, warn};

use super::attrs::{BLKSIZE, GENERATION, TTL, file_kind_to_fuse, make_attr, resolve_attr_ttl};
use super::handles::{OpenMode, WriteOutcome};
use super::inode_map::{InodeEntry, ROOT_INODE};
use super::mode::{PermissionsExt, WRITE_BITS};
use super::{FuseFilesystem, ensure_dir_path, fuse_err, fuse_try, reply_enotsup};
use crate::err::extract_errno;
use crate::router::fs::mode as fs_mode;
use crate::router::{NamedNode, Permissions, Process};
use crate::types::Timestamps;
use crate::types::file_kind::FileKind;
/// Controls whether [`FuseFilesystem::do_flush`] defers empty truncations.
#[derive(Debug, Clone, Copy)]
enum FlushMode {
    /// Mid-life flush (from `flush` callback): defer empty `O_TRUNC` buffers
    /// so write data can arrive before the content is spliced.
    Eager,
    /// Final flush (from `release` callback): always flush, even if the
    /// buffer is empty from truncation.
    Final,
}

impl FuseFilesystem {
    /// Resolve attributes for an inode via chain dispatch.
    ///
    /// Dispatches a lookup to resolve the node, then delegates to
    /// [`node_attr`] for attr construction. Content is never read here —
    /// size becomes accurate only after a FUSE `read` populates the
    /// [`CachedReadable`](nyne_cache) cache, at which point
    /// [`Readable::size`] returns the real length.
    pub(super) fn resolve_attr(&self, ino: u64, req: &Request) -> Option<(fuser::FileAttr, Duration)> {
        if ino == ROOT_INODE {
            return Some((self.root_attr(req), TTL));
        }
        let entry = self.inodes.get(ino)?;
        let node = self
            .lookup_node(&entry.dir_path, &entry.name, Some(self.process_from(req)))
            .ok()??;
        Some(self.node_attr(ino, &node, req))
    }

    /// Build attrs from a pre-resolved `NamedNode` (no chain dispatch).
    ///
    /// Cheap path: uses [`Readable::size`] if cached, falls back to
    /// [`BLKSIZE`] for readable virtual files with unknown size. Write-only
    /// nodes (no `Readable`) report size 0. Used by `readdirplus` where
    /// reading content for every entry is too expensive.
    ///
    /// **TTL** is sourced from the node's [`CachePolicy`] when set
    /// (`NoCache` → `Duration::ZERO`, `Ttl(d)` → `d`); otherwise
    /// (`Default`) it falls back to a per-file-type heuristic: real
    /// files get [`TTL`] (1s), virtual nodes get `Duration::ZERO` to
    /// force kernel re-validation on every access.
    pub(super) fn node_attr(&self, ino: u64, node: &NamedNode, req: &Request) -> (fuser::FileAttr, Duration) {
        // Real files: use backing filesystem metadata for accurate size/mtime.
        if let Some(backing) = node.readable().and_then(|r| r.backing_path())
            && let Ok(meta) = self.backing_fs.metadata(backing)
        {
            return (
                make_attr(
                    ino,
                    meta.size,
                    file_kind_to_fuse(FileKind::from(node.kind())),
                    fs_mode::narrow(meta.permissions, fs_mode::FILE_DEFAULT),
                    self.inode_state.atime(ino).map_or(meta.timestamps, |atime| Timestamps {
                        atime,
                        ..meta.timestamps
                    }),
                    req,
                ),
                resolve_attr_ttl(node.cache_policy(), TTL),
            );
        }

        // Virtual files: use cached size if available, BLKSIZE fallback for
        // readable files with unknown size. Write-only nodes (no `Readable`)
        // report size 0 — this matches their semantics (no content to read)
        // and avoids triggering host-side "file exists" heuristics that
        // would otherwise gate writes on a prior read. This is what makes
        // batch edit action files under `edit/{op}` writable without a
        // preceding read.
        let kind = FileKind::from(node.kind());
        (
            make_attr(
                ino,
                node.readable().map_or(0, |r| {
                    r.size()
                        .unwrap_or_else(|| if kind == FileKind::File { u64::from(BLKSIZE) } else { 0 })
                }),
                file_kind_to_fuse(kind),
                node.permissions().to_mode_bits(),
                node.timestamps(),
                req,
            ),
            resolve_attr_ttl(node.cache_policy(), Duration::ZERO),
        )
    }

    /// Build a `FileAttr` for the root directory from real filesystem metadata.
    fn root_attr(&self, req: &Request) -> fuser::FileAttr {
        let (size, ts, perm) = self.backing_fs.metadata(Path::new("")).map_or_else(
            |_| (0, Timestamps::default(), fs_mode::DIR_FALLBACK),
            |m| {
                (
                    m.size,
                    m.timestamps,
                    fs_mode::narrow(m.permissions, fs_mode::DIR_DEFAULT),
                )
            },
        );
        make_attr(ROOT_INODE, size, fuser::FileType::Directory, perm, ts, req)
    }

    /// Reply with attrs for an inode, or ENOENT.
    pub(super) fn reply_attr(&self, ino: u64, req: &Request, reply: ReplyAttr) {
        match self.resolve_attr(ino, req) {
            Some((attr, ttl)) => reply.attr(&ttl, &attr),
            None => reply.error(Errno::ENOENT),
        }
    }

    /// Allocate or find an existing inode for a (`dir_path`, name) pair.
    pub(super) fn ensure_inode(&self, dir_path: &Path, name: &str, parent_inode: u64) -> u64 {
        self.inodes.find_inode(dir_path, name).unwrap_or_else(|| {
            self.inodes
                .allocate(InodeEntry::new(dir_path.to_path_buf(), name.to_owned(), parent_inode))
        })
    }

    /// Resolve a child entry: lookup via the chain, allocate an inode if found.
    ///
    /// Combines [`lookup_node`](Self::lookup_node) and
    /// [`ensure_inode`](Self::ensure_inode) into a single call, returning
    /// both the stable inode number and the resolved node. Callers can then
    /// pass the node directly to [`node_attr`](Self::node_attr) without a
    /// redundant second chain dispatch.
    pub(super) fn resolve_inode(
        &self,
        dir_path: &Path,
        name: &str,
        parent_inode: u64,
        process: Option<Process>,
    ) -> eyre::Result<Option<(u64, NamedNode)>> {
        let Some(node) = self.lookup_node(dir_path, name, process)? else {
            return Ok(None);
        };
        let ino = self.ensure_inode(dir_path, name, parent_inode);
        Ok(Some((ino, node)))
    }

    /// Check whether a parent directory inode permits child mutations.
    ///
    /// Returns `true` if the directory's permission bits include owner-write.
    /// Used by mutation handlers (create, mkdir, remove, rename) to reject
    /// operations in read-only directories before dispatching through the chain.
    pub(super) fn is_writable_dir(&self, ino: u64, req: &Request) -> bool {
        self.resolve_attr(ino, req)
            .is_some_and(|(attr, _)| attr.perm & WRITE_BITS != 0)
    }

    /// Execute the flush pipeline for a single file handle.
    ///
    /// Acquires the per-inode write lock, snapshots the dirty buffer,
    /// writes through the chain, and updates both the handle's dirty
    /// generation and the inode error map atomically.
    ///
    /// Returns `Ok(())` whether the handle was already clean, the dirty
    /// buffer was deferred (`O_TRUNC` guard in [`FlushMode::Eager`]),
    /// or the flush succeeded — callers don't distinguish these cases.
    /// `Err(e)` carries the flush failure for errno mapping.
    ///
    /// The `O_TRUNC` deferral guard is only applied in [`FlushMode::Eager`]
    /// (from the `flush` callback). [`FlushMode::Final`] (from `release`)
    /// always flushes, even if the buffer is empty from truncation.
    fn do_flush(&self, ino: u64, fh: u64, mode: FlushMode) -> Result<(), eyre::Error> {
        let Some(snapshot) = self.handles.dirty_snapshot(fh) else {
            debug!(target: "nyne::fuse", ino, fh, ?mode, "do_flush: not dirty");
            return Ok(());
        };
        debug!(target: "nyne::fuse", ino, fh, ?mode, data_len = snapshot.data.len(), truncated = snapshot.truncated, gen = snapshot.generation, "do_flush: dirty snapshot");

        // The Linux FUSE kernel module handles O_TRUNC by stripping the
        // flag from open and sending setattr(size=0) + flush BEFORE the
        // write data arrives. Without this guard, the empty-buffer flush
        // would splice "" into the source file, destroying the symbol
        // before the actual content arrives.
        //
        // Deferring to release is safe: if writes follow (echo > file),
        // the next flush sends the actual data. If no writes follow
        // (: > file), release flushes the empty truncation.
        if matches!(mode, FlushMode::Eager) && snapshot.data.is_empty() && snapshot.truncated {
            trace!(target: "nyne::fuse", ino, fh, "flush: deferring empty truncation to release");
            return Ok(());
        }

        // Per-inode write lock prevents concurrent flushes.
        let lock = self.inode_state.write_lock(ino);
        let _guard = lock.lock();

        match self.flush_content(ino, &snapshot.data) {
            Ok(()) => {
                self.handles.clear_dirty(fh, snapshot.generation);
                self.inode_state.clear_write_error(ino);
                Ok(())
            }
            Err(e) => {
                debug!(target: "nyne::fuse", ino, error = %e, "flush failed");
                self.inode_state.set_write_error(ino, format!("{e:#}"));
                Err(e)
            }
        }
    }
}

impl Filesystem for FuseFilesystem {
    fn init(&mut self, _req: &Request, config: &mut fuser::KernelConfig) -> io::Result<()> {
        if let Err(unsupported) = config.add_capabilities(InitFlags::FUSE_PASSTHROUGH) {
            warn!(target: "nyne::fuse", ?unsupported, "kernel does not support FUSE_PASSTHROUGH");
        } else {
            match config.set_max_stack_depth(1) {
                Ok(_) => {
                    self.passthrough_enabled.store(true, Ordering::Relaxed);
                    info!(target: "nyne::fuse", "passthrough enabled (max_stack_depth=1)");
                }
                Err(nearest) => {
                    warn!(target: "nyne::fuse", nearest, "kernel rejected max_stack_depth=1");
                }
            }
        }
        if let Err(unsupported) =
            config.add_capabilities(InitFlags::FUSE_DO_READDIRPLUS | InitFlags::FUSE_READDIRPLUS_AUTO)
        {
            warn!(target: "nyne::fuse", ?unsupported, "kernel does not support READDIRPLUS");
        }
        info!(target: "nyne::fuse", "FUSE filesystem initialized");
        Ok(())
    }

    fn lookup(&self, req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        let parent = u64::from(parent);
        let dir_path = ensure_dir_path!(self, parent, reply);
        let name = name.to_string_lossy();
        trace!(target: "nyne::fuse", parent, path = %dir_path.display(), name = %name, "lookup");

        match self.resolve_inode(&dir_path, &name, parent, Some(self.process_from(req))) {
            Ok(Some((ino, node))) => {
                let (attr, ttl) = self.node_attr(ino, &node, req);
                reply.entry(&ttl, &attr, GENERATION);
            }
            Ok(None) => {
                trace!(target: "nyne::fuse", parent, name = %name, "lookup: not found");
                reply.error(Errno::ENOENT);
            }
            Err(e) => reply.error(extract_errno(&e)),
        }
    }

    fn getattr(&self, req: &Request, ino: INodeNo, _fh: Option<FileHandle>, reply: ReplyAttr) {
        self.reply_attr(u64::from(ino), req, reply);
    }

    fn setattr(
        &self,
        req: &Request,
        ino: INodeNo,
        _mode: Option<u32>,
        _uid: Option<u32>,
        _gid: Option<u32>,
        size: Option<u64>,
        atime: Option<TimeOrNow>,
        _mtime: Option<TimeOrNow>,
        _ctime: Option<SystemTime>,
        fh: Option<FileHandle>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<SystemTime>,
        _flags: Option<BsdFileFlags>,
        reply: ReplyAttr,
    ) {
        let ino = u64::from(ino);

        if let Some(new_size) = size {
            if let Some(fh) = fh {
                self.handles.truncate(u64::from(fh), new_size);
            } else {
                self.handles.truncate_by_inode(ino, new_size);
            }
        }

        if let Some(t) = atime {
            self.inode_state.set_atime(ino, match t {
                TimeOrNow::SpecificTime(t) => t,
                TimeOrNow::Now => SystemTime::now(),
            });
        }

        self.reply_attr(ino, req, reply);
    }

    /// List directory entries for a directory inode.
    ///
    /// See [`readdir_offset`] for the offset protocol.
    fn readdir(&self, req: &Request, ino: INodeNo, _fh: FileHandle, offset: u64, mut reply: fuser::ReplyDirectory) {
        let ino = u64::from(ino);
        let dir_path = ensure_dir_path!(self, ino, reply);
        let nodes = fuse_try!(
            reply,
            self.read_dir_nodes(&dir_path, Some(self.process_from(req))),
            ino,
            "readdir failed"
        );

        let parent = self.inodes.parent_of(ino);
        if offset < OFFSET_DOT && reply.add(INodeNo(ino), OFFSET_DOT, fuser::FileType::Directory, ".") {
            reply.ok();
            return;
        }
        if offset < OFFSET_DOTDOT && reply.add(INodeNo(parent), OFFSET_DOTDOT, fuser::FileType::Directory, "..") {
            reply.ok();
            return;
        }

        for (i, node) in nodes.iter().enumerate().skip(readdir_skip(offset)) {
            let child_ino = self.ensure_inode(&dir_path, node.name(), ino);
            if reply.add(
                INodeNo(child_ino),
                readdir_offset(i),
                file_kind_to_fuse(FileKind::from(node.kind())),
                node.name(),
            ) {
                break;
            }
        }
        reply.ok();
    }

    /// List directory entries with attributes. Same offset protocol as
    /// [`readdir`](Self::readdir). Uses [`node_attr`](FuseFilesystem::build_node_attr)
    /// with the already-resolved nodes to avoid per-entry chain dispatches.
    fn readdirplus(
        &self,
        req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        mut reply: fuser::ReplyDirectoryPlus,
    ) {
        let ino = u64::from(ino);
        let dir_path = ensure_dir_path!(self, ino, reply);
        let nodes = fuse_try!(
            reply,
            self.read_dir_nodes(&dir_path, Some(self.process_from(req))),
            ino,
            "readdirplus failed"
        );

        let parent = self.inodes.parent_of(ino);
        if offset < OFFSET_DOT
            && let Some((attr, ttl)) = self.resolve_attr(ino, req)
            && reply.add(INodeNo(ino), OFFSET_DOT, ".", &ttl, &attr, GENERATION)
        {
            reply.ok();
            return;
        }
        if offset < OFFSET_DOTDOT
            && let Some((attr, ttl)) = self.resolve_attr(parent, req)
            && reply.add(INodeNo(parent), OFFSET_DOTDOT, "..", &ttl, &attr, GENERATION)
        {
            reply.ok();
            return;
        }

        for (i, node) in nodes.iter().enumerate().skip(readdir_skip(offset)) {
            let child_ino = self.ensure_inode(&dir_path, node.name(), ino);
            let (attr, ttl) = self.node_attr(child_ino, node, req);
            if reply.add(
                INodeNo(child_ino),
                readdir_offset(i),
                node.name(),
                &ttl,
                &attr,
                GENERATION,
            ) {
                break;
            }
        }
        reply.ok();
    }

    fn open(&self, req: &Request, ino: INodeNo, flags: OpenFlags, reply: ReplyOpen) {
        let ino = u64::from(ino);
        trace!(target: "nyne::fuse", ino, "open");

        // Resolve the node once for all open paths.
        let process = Some(self.process_from(req));
        let entry = self.inodes.get(ino);
        let node = entry
            .as_ref()
            .and_then(|e| self.lookup_node(&e.dir_path, &e.name, process.clone()).ok().flatten());
        // Resolve backing file path for passthrough/direct-fd I/O.
        // backing_path() returns a path relative to the filesystem root;
        // source_dir().join() makes it absolute for std::fs::File::open.
        let backing_file = node.as_ref().and_then(|n| {
            let rel = n.readable()?.backing_path()?;
            let abs = self.backing_fs.source_dir().join(rel);
            File::open(&abs).ok()
        });

        // Try FUSE kernel passthrough for real files.
        if let Some(file) = backing_file.as_ref()
            && self.passthrough_enabled.load(Ordering::Relaxed)
        {
            match reply.open_backing(file) {
                Ok(backing_id) => {
                    trace!(target: "nyne::fuse", ino, "open: passthrough");
                    reply.opened_passthrough(FileHandle(0), FopenFlags::empty(), &backing_id);
                    return;
                }
                Err(e) => {
                    self.passthrough_enabled.store(false, Ordering::Relaxed);
                    info!(target: "nyne::fuse", ino, error = %e,
                            "passthrough unavailable, falling back to buffered I/O");
                }
            }
        }

        // Direct fd path: pread()-based I/O for real files. Avoids loading
        // large files (e.g., git pack files) into memory.
        //
        // FOPEN_KEEP_CACHE (not FOPEN_DIRECT_IO) is used here so the kernel
        // page cache backs mmap(). libgit2's pack reader mmaps .pack/.idx
        // files unconditionally; FOPEN_DIRECT_IO would make mmap fail with
        // ENODEV. Our userspace still reads via pread() — only the kernel's
        // caching behavior changes.
        let open_flags = flags.0;
        let mode = OpenMode::parse(open_flags);
        if let Some(file) = backing_file
            && !mode.truncate
        {
            let fh = self.handles.open_direct(ino, file, open_flags);
            reply.opened(FileHandle(fh), FopenFlags::FOPEN_KEEP_CACHE);
            return;
        }

        // Write permission check.
        if mode.write_intent
            && let Some(ref n) = node
            && n.writable().is_none()
        {
            fuse_err!(reply, Errno::EACCES, ino, "open: write rejected (not writable)");
        }

        if let Some(ref n) = node {
            self.notify_open(ino, n);
        }

        // Buffered path: load content into handle table.
        // Skip read when truncating (content will be discarded) or when the
        // node has no readable (write-only nodes like batch edit staging).
        let has_readable = node.as_ref().is_some_and(|n| n.readable().is_some());
        let content: Arc<[u8]> = if mode.truncate || !has_readable {
            Arc::from([])
        } else {
            fuse_try!(reply, self.load_content(ino), ino, "open failed")
        };
        let fh = self.handles.open(ino, content, open_flags);
        reply.opened(FileHandle(fh), FopenFlags::FOPEN_DIRECT_IO);
    }

    fn read(
        &self,
        _req: &Request,
        ino: INodeNo,
        fh: FileHandle,
        offset: u64,
        size: u32,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyData,
    ) {
        let ino = u64::from(ino);
        match self.handles.read(u64::from(fh), offset, size) {
            Ok(data) => reply.data(&data),
            Err(e) => {
                debug!(target: "nyne::fuse", ino, error = %e, "read failed");
                reply.error(Errno::EIO);
            }
        }
    }

    fn write(
        &self,
        _req: &Request,
        ino: INodeNo,
        fh: FileHandle,
        offset: u64,
        data: &[u8],
        _write_flags: WriteFlags,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyWrite,
    ) {
        let ino = u64::from(ino);
        let fh = u64::from(fh);
        match self.handles.write(fh, offset, data) {
            Some(WriteOutcome::Buffered(n)) => reply.written(n),
            Some(WriteOutcome::Replacement(n)) => {
                // First write after O_TRUNC — eagerly flush to surface
                // validation errors on the write() syscall itself, rather
                // than deferring to close() where shells discard the error.
                match self.do_flush(ino, fh, FlushMode::Final) {
                    Err(e) => reply.error(extract_errno(&e)),
                    Ok(()) => reply.written(n),
                }
            }
            None => {
                debug!(target: "nyne::fuse", ino, "write failed");
                reply.error(Errno::EIO);
            }
        }
    }

    fn flush(&self, _req: &Request, ino: INodeNo, fh: FileHandle, _lock_owner: LockOwner, reply: ReplyEmpty) {
        match self.do_flush(u64::from(ino), u64::from(fh), FlushMode::Eager) {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(extract_errno(&e)),
        }
    }

    fn release(
        &self,
        _req: &Request,
        ino: INodeNo,
        fh: FileHandle,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        let ino = u64::from(ino);
        let fh = u64::from(fh);

        // Flush remaining dirty data (Final = never defer truncations).
        // Errors are recorded in write_errors (surfaced via user.error xattr)
        // and discarded here — release cannot report errors to userspace.
        let _ = self.do_flush(ino, fh, FlushMode::Final);

        self.handles.release(fh);

        // Last-handle cleanup: lifecycle hook + per-inode state eviction.
        if !self.handles.has_handles_for_inode(ino) {
            if let Some(entry) = self.inodes.get(ino)
                && let Ok(Some(node)) = self.lookup_node(&entry.dir_path, &entry.name, None)
            {
                self.notify_close(ino, &node);
            }

            // Evict per-inode state to prevent unbounded growth.
            self.inode_state.evict(ino);
        }

        reply.ok();
    }

    fn opendir(&self, _req: &Request, _ino: INodeNo, _flags: OpenFlags, reply: ReplyOpen) {
        reply.opened(FileHandle(0), FopenFlags::empty());
    }

    fn releasedir(&self, _req: &Request, _ino: INodeNo, _fh: FileHandle, _flags: OpenFlags, reply: ReplyEmpty) {
        reply.ok();
    }

    fn access(&self, _req: &Request, ino: INodeNo, mask: AccessFlags, reply: ReplyEmpty) {
        let ino = u64::from(ino);
        trace!(target: "nyne::fuse", ino, ?mask, "access");

        if ino == ROOT_INODE {
            reply.ok();
            return;
        }

        let Some(entry) = self.inodes.get(ino) else {
            fuse_err!(reply, Errno::ENOENT, ino, "access: inode not found");
        };
        let Some(node) = self.lookup_node(&entry.dir_path, &entry.name, None).ok().flatten() else {
            fuse_err!(reply, Errno::ENOENT, ino, "access: node not found");
        };

        // Check requested access mask against the node's capability flags directly,
        // bypassing the FUSE mode-bit round-trip.
        let perms = node.permissions();
        if mask.contains(AccessFlags::R_OK) && !perms.contains(Permissions::READ) {
            fuse_err!(reply, Errno::EACCES, ino, "access: R_OK denied");
        }
        if mask.contains(AccessFlags::W_OK) && !perms.contains(Permissions::WRITE) {
            fuse_err!(reply, Errno::EACCES, ino, "access: W_OK denied");
        }
        if mask.contains(AccessFlags::X_OK) && !perms.contains(Permissions::EXECUTE) {
            fuse_err!(reply, Errno::EACCES, ino, "access: X_OK denied");
        }
        reply.ok();
    }

    fn readlink(&self, _req: &Request, ino: INodeNo, reply: ReplyData) {
        let ino = u64::from(ino);
        let Some(entry) = self.inodes.get(ino) else {
            fuse_err!(reply, Errno::ENOENT, ino, "readlink: inode not found");
        };
        match self.lookup_node(&entry.dir_path, &entry.name, None) {
            Ok(Some(node)) => match node.target() {
                Some(target) => reply.data(target.as_os_str().as_encoded_bytes()),
                None => reply.error(Errno::EINVAL),
            },
            Ok(None) => reply.error(Errno::ENOENT),
            Err(e) => reply.error(extract_errno(&e)),
        }
    }

    fn create(
        &self,
        req: &Request,
        parent: INodeNo,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        flags: i32,
        reply: ReplyCreate,
    ) {
        self.do_create(req, parent, name, flags, reply);
    }

    fn mkdir(&self, req: &Request, parent: INodeNo, name: &OsStr, _mode: u32, _umask: u32, reply: ReplyEntry) {
        self.do_mkdir(req, parent, name, reply);
    }

    fn unlink(&self, req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEmpty) {
        self.do_remove(req, parent, name, reply);
    }

    fn rmdir(&self, req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEmpty) {
        self.do_remove(req, parent, name, reply);
    }

    fn rename(
        &self,
        req: &Request,
        parent: INodeNo,
        name: &OsStr,
        newparent: INodeNo,
        newname: &OsStr,
        _flags: RenameFlags,
        reply: ReplyEmpty,
    ) {
        self.do_rename(req, parent, name, newparent, newname, reply);
    }

    fn getxattr(&self, _req: &Request, ino: INodeNo, name: &OsStr, size: u32, reply: ReplyXattr) {
        self.do_getxattr(ino, name, size, reply);
    }

    fn listxattr(&self, _req: &Request, ino: INodeNo, size: u32, reply: ReplyXattr) {
        self.do_listxattr(ino, size, reply);
    }

    fn setxattr(
        &self,
        _req: &Request,
        ino: INodeNo,
        name: &OsStr,
        value: &[u8],
        _flags: i32,
        _position: u32,
        reply: ReplyEmpty,
    ) {
        self.do_setxattr(ino, name, value, reply);
    }

    fn removexattr(&self, _req: &Request, _ino: INodeNo, _name: &OsStr, reply: ReplyEmpty) {
        reply_enotsup!(reply, "removexattr");
    }

    fn destroy(&mut self) {
        info!(target: "nyne::fuse", "FUSE session destroyed");
    }

    fn forget(&self, _req: &Request, _ino: INodeNo, _nlookup: u64) {}

    #[expect(clippy::cast_possible_truncation, reason = "fuser API requires u32")]
    fn statfs(&self, _req: &Request, _ino: INodeNo, reply: ReplyStatfs) {
        match statvfs(self.backing_fs.source_dir()) {
            Ok(st) => reply.statfs(
                st.f_blocks,
                st.f_bfree,
                st.f_bavail,
                st.f_files,
                st.f_ffree,
                st.f_bsize as u32,
                st.f_namemax as u32,
                st.f_frsize as u32,
            ),
            Err(e) => {
                warn!(target: "nyne::fuse", error = %e, "statfs failed");
                reply.error(Errno::EIO);
            }
        }
    }

    fn mknod(
        &self,
        _req: &Request,
        _parent: INodeNo,
        _name: &OsStr,
        _mode: u32,
        _umask: u32,
        _rdev: u32,
        reply: ReplyEntry,
    ) {
        reply_enotsup!(reply, "mknod");
    }

    fn symlink(&self, _req: &Request, _parent: INodeNo, _link_name: &OsStr, _target: &Path, reply: ReplyEntry) {
        reply_enotsup!(reply, "symlink");
    }

    fn link(&self, _req: &Request, _ino: INodeNo, _newparent: INodeNo, _newname: &OsStr, reply: ReplyEntry) {
        reply_enotsup!(reply, "link");
    }

    fn fsync(&self, _req: &Request, _ino: INodeNo, _fh: FileHandle, _datasync: bool, reply: ReplyEmpty) { reply.ok(); }

    fn fsyncdir(&self, _req: &Request, _ino: INodeNo, _fh: FileHandle, _datasync: bool, reply: ReplyEmpty) {
        reply.ok();
    }
}

/// FUSE readdir offset protocol.
///
/// The kernel calls readdir/readdirplus repeatedly to paginate results.
/// Each `reply.add(ino, offset, ...)` associates an opaque offset cookie
/// with the entry. On the next call, the kernel passes back the offset of
/// the **last entry it accepted**; we must emit only entries **after** that
/// offset. If `reply.add` returns `true`, the reply buffer is full — the
/// kernel will call back with the last accepted offset.
///
/// We use a simple sequential numbering scheme:
///
/// | Offset | Entry |
/// |-|-|
/// | `OFFSET_DOT` (1) | `.` (self) |
/// | `OFFSET_DOTDOT` (2) | `..` (parent) |
/// | `OFFSET_ENTRIES` (3) | first real entry |
/// | `OFFSET_ENTRIES + N` | Nth real entry |
///
/// `offset == 0` means "start from the beginning".
///
/// **Invariant:** offsets must be stable across calls for the same
/// directory contents. `read_dir_nodes` returns a consistent ordered
/// list per dispatch, so sequential indexing satisfies this.
const OFFSET_DOT: u64 = 1;
const OFFSET_DOTDOT: u64 = 2;
const OFFSET_ENTRIES: u64 = 3;

/// Convert a node index (0-based position in the `read_dir_nodes` result)
/// to a FUSE readdir offset cookie.
const fn readdir_offset(index: usize) -> u64 { OFFSET_ENTRIES + index as u64 }

/// How many entries to skip given the kernel's last-accepted offset.
///
/// The kernel passes 0 on the first call. After accepting entry at
/// offset N, it passes N on the next call. We must skip all entries
/// with offset <= N.
const fn readdir_skip(last_offset: u64) -> usize {
    if last_offset < OFFSET_ENTRIES {
        0
    } else {
        #[allow(clippy::cast_possible_truncation)] // readdir entry count never exceeds usize
        {
            (last_offset - OFFSET_ENTRIES) as usize + 1
        }
    }
}
