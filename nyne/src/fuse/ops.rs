//! Fuser `Filesystem` trait implementation — all FUSE protocol callbacks.

use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::SystemTime;

use fuser::{
    AccessFlags, BsdFileFlags, CopyFileRangeFlags, Errno, FileHandle, Filesystem, FopenFlags, INodeNo, InitFlags,
    IoctlFlags, LockOwner, OpenFlags, RenameFlags, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory,
    ReplyDirectoryPlus, ReplyEmpty, ReplyEntry, ReplyIoctl, ReplyLseek, ReplyOpen, ReplyStatfs, ReplyWrite, ReplyXattr,
    Request, WriteFlags,
};
use rustix::fs::statvfs;
use tracing::{debug, info, trace, warn};

use super::{NyneFs, extract_errno, file_kind_to_fuse};
use crate::dispatch::{ResolvedInode, WriteMode};
use crate::node::NodeKind;
use crate::types::ProcessVisibility;
use crate::types::file_kind::FileKind;

/// `Filesystem` implementation for [`NyneFs`], dispatching FUSE protocol callbacks.
impl Filesystem for NyneFs {
    /// Initializes the FUSE filesystem, negotiating kernel capabilities.
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
                    warn!(target: "nyne::fuse", nearest, "kernel rejected max_stack_depth=1, passthrough unavailable");
                }
            }
        }
        if let Err(unsupported) =
            config.add_capabilities(InitFlags::FUSE_DO_READDIRPLUS | InitFlags::FUSE_READDIRPLUS_AUTO)
        {
            warn!(target: "nyne::fuse", ?unsupported, "kernel does not support READDIRPLUS");
        } else {
            info!(target: "nyne::fuse", "READDIRPLUS enabled");
        }

        info!(target: "nyne::fuse", "FUSE filesystem initialized");
        Ok(())
    }

    /// Looks up a child entry by name within a parent directory.
    fn lookup(&self, req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        with_parent_ctx!(self, parent, name, reply, "lookup", |parent_ino, name_str, ctx| {
            // None-visibility processes see only real filesystem entries.
            let result = if self.process_visibility(req) == ProcessVisibility::None {
                fuse_try!(reply, self.router.lookup_real(&name_str, &ctx),
                    parent_ino, name = %name_str, "lookup failed")
            } else {
                fuse_try!(reply, self.router.lookup_name(&name_str, &ctx),
                    parent_ino, name = %name_str, "lookup failed")
            };
            if let Some(inode) = result {
                self.reply_entry(inode, req, reply);
            } else {
                trace!(target: "nyne::fuse", parent_ino, name = %name_str, "lookup: not found");
                reply.error(Errno::ENOENT);
            }
        });
    }

    /// Returns file attributes for an inode.
    fn getattr(&self, req: &Request, ino: INodeNo, _fh: Option<FileHandle>, reply: ReplyAttr) {
        let ino = u64::from(ino);
        trace!(target: "nyne::fuse", ino, "getattr");
        self.reply_attr(ino, req, reply);
    }

    /// Sets file attributes (size, timestamps) for an inode.
    fn setattr(
        &self,
        req: &Request,
        ino: INodeNo,
        _mode: Option<u32>,
        _uid: Option<u32>,
        _gid: Option<u32>,
        size: Option<u64>,
        atime: Option<fuser::TimeOrNow>,
        _mtime: Option<fuser::TimeOrNow>,
        _ctime: Option<SystemTime>,
        fh: Option<FileHandle>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<SystemTime>,
        _flags: Option<BsdFileFlags>,
        reply: ReplyAttr,
    ) {
        let ino = u64::from(ino);
        debug!(target: "nyne::fuse", ino, size, ?atime, "setattr");

        if let Some(new_size) = size {
            if let Some(fh) = fh {
                self.handles.truncate(u64::from(fh), new_size);
            } else {
                self.handles.truncate_by_inode(ino, new_size);
            }
        }

        if let Some(atime_val) = atime {
            let ts = match atime_val {
                fuser::TimeOrNow::SpecificTime(t) => t,
                fuser::TimeOrNow::Now => SystemTime::now(),
            };
            self.atime_overrides.write().insert(ino, ts);
        }

        self.reply_attr(ino, req, reply);
    }

    /// Lists directory entries for a directory inode.
    fn readdir(&self, req: &Request, ino: INodeNo, _fh: FileHandle, offset: u64, mut reply: ReplyDirectory) {
        let ino = u64::from(ino);
        let dir_path = ensure_dir_path!(self, ino, reply);
        let vis = self.process_visibility(req);
        debug!(target: "nyne::fuse", ino, offset, path = %dir_path, %vis, "readdir");

        if vis == ProcessVisibility::None {
            self.for_each_real_readdir_entry(&dir_path, ino, offset, |entry_ino, next_offset, entry| {
                reply.add(
                    INodeNo(entry_ino),
                    next_offset,
                    file_kind_to_fuse(entry.kind),
                    &entry.name,
                )
            });
            reply.ok();
            return;
        }

        let ctx = self.router.make_request_context(&dir_path);
        fuse_try!(reply, self.router.ensure_resolved(&ctx), ino, "readdir: resolve failed");

        self.for_each_readdir_entry(&dir_path, ino, offset, vis, |entry_ino, next_offset, entry| {
            reply.add(
                INodeNo(entry_ino),
                next_offset,
                file_kind_to_fuse(entry.kind),
                &entry.name,
            )
        });
        reply.ok();
    }

    /// Lists directory entries with attributes for a directory inode.
    fn readdirplus(&self, req: &Request, ino: INodeNo, _fh: FileHandle, offset: u64, mut reply: ReplyDirectoryPlus) {
        let ino = u64::from(ino);
        let dir_path = ensure_dir_path!(self, ino, reply);
        let vis = self.process_visibility(req);
        debug!(target: "nyne::fuse", ino, offset, path = %dir_path, %vis, "readdirplus");

        if vis == ProcessVisibility::None {
            self.for_each_real_readdir_entry(&dir_path, ino, offset, |entry_ino, next_offset, entry| {
                self.add_dirplus_entry(&mut reply, entry_ino, next_offset, entry, req)
            });
            reply.ok();
            return;
        }

        let ctx = self.router.make_request_context(&dir_path);
        fuse_try!(
            reply,
            self.router.ensure_resolved(&ctx),
            ino,
            "readdirplus: resolve failed"
        );

        self.for_each_readdir_entry(&dir_path, ino, offset, vis, |entry_ino, next_offset, entry| {
            self.add_dirplus_entry(&mut reply, entry_ino, next_offset, entry, req)
        });
        reply.ok();
    }

    /// Opens a file, setting up buffered or passthrough I/O.
    fn open(&self, req: &Request, ino: INodeNo, flags: OpenFlags, reply: ReplyOpen) {
        let ino = u64::from(ino);
        let resolved = self.resolve_for_request(ino, req);
        debug!(target: "nyne::fuse", ino, path = resolved.as_ref().map_or("?", ResolvedInode::path_str), "open");

        // Try FUSE kernel passthrough for real files.
        if self.passthrough_enabled.load(Ordering::Relaxed)
            && let Some(ResolvedInode::Real { ref path, .. }) = resolved
            && let Some(file) = self.router.real_fs().open_raw(path)
        {
            match reply.open_backing(&file) {
                Ok(backing_id) => {
                    trace!(target: "nyne::fuse", ino, "open: passthrough");
                    reply.opened_passthrough(FileHandle(0), FopenFlags::empty(), &backing_id);
                    return;
                }
                Err(e) => {
                    // Passthrough not usable — disable to avoid repeated syscalls.
                    // The kernel requires CAP_SYS_ADMIN for FUSE_DEV_IOC_BACKING_OPEN.
                    self.passthrough_enabled.store(false, Ordering::Relaxed);
                    info!(target: "nyne::fuse", path = %path, error = %e,
                        "passthrough unavailable (requires CAP_SYS_ADMIN), falling back to buffered I/O");
                }
            }
        }

        // Direct fd path: real files get pread()-based I/O when kernel
        // passthrough is unavailable. Avoids loading entire file contents
        // into memory (critical for large files like git pack files).
        let open_flags = flags.0;
        let mode = super::handles::OpenMode::parse(open_flags);
        if let Some(ResolvedInode::Real { ref path, .. }) = resolved
            && !mode.truncate
            && let Some(file) = self.router.real_fs().open_raw(path)
        {
            let fh = self.handles.open_direct(ino, file, open_flags);
            reply.opened(FileHandle(fh), FopenFlags::FOPEN_DIRECT_IO);
            return;
        }

        // Virtual-node permission check: reject write opens on nodes
        // without the Writable capability. Failing here (instead of at
        // flush time) ensures the shell redirect `> file` returns EACCES
        // before the command runs — shells don't propagate close() errors.
        if mode.write_intent
            && let Some(ResolvedInode::Virtual { ref node, .. }) = resolved
            && node.writable().is_none()
        {
            debug!(target: "nyne::fuse", ino, name = node.name(), "open: write rejected (not writable)");
            reply.error(Errno::EACCES);
            return;
        }

        // Call lifecycle open hook for virtual nodes.
        if let Some(ResolvedInode::Virtual {
            ref node, ref dir_path, ..
        }) = resolved
            && let Some(lc) = node.lifecycle()
        {
            let ctx = self.router.make_request_context(dir_path);
            fuse_try!(reply, lc.open(&ctx), ino, "lifecycle open failed");
        }

        // Buffered path: load content into handle table.
        // Skip the read pipeline for O_TRUNC — HandleTable::open discards
        // the content anyway, so loading it is pure waste.
        let content = if mode.truncate {
            Vec::new()
        } else {
            fuse_try!(reply, self.load_content(ino), ino, "open failed")
        };
        let fh = self.handles.open(ino, content, open_flags);
        reply.opened(FileHandle(fh), FopenFlags::FOPEN_DIRECT_IO);
    }

    /// Reads data from an open file handle at the given offset.
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
        let (ino, fh) = (u64::from(ino), u64::from(fh));
        trace!(target: "nyne::fuse", ino, fh, offset, size, "read");

        let data = self.handles.read(fh, offset, size);
        reply.data(&data);
    }

    /// Writes data to an open file handle at the given offset.
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
        debug!(target: "nyne::fuse", ino, fh, offset, size = data.len(), "write");

        match self.handles.write(fh, offset, data) {
            Some(written) => reply.written(written),
            None => reply.error(Errno::EIO),
        }
    }

    /// Flushes dirty buffer contents for an open file handle.
    fn flush(&self, _req: &Request, ino: INodeNo, fh: FileHandle, _lock_owner: LockOwner, reply: ReplyEmpty) {
        let (ino, fh) = (u64::from(ino), u64::from(fh));
        trace!(target: "nyne::fuse", ino, fh, "flush");

        if let Some((data, mode, dirty_gen)) = self.handles.dirty_snapshot(fh) {
            // The Linux FUSE kernel module handles O_TRUNC by stripping
            // the flag from open and sending setattr(size=0) + flush
            // BEFORE the write data arrives. Without this guard, the
            // empty-buffer flush would splice "" into the source file,
            // destroying the symbol before the actual content arrives.
            //
            // Deferring to release is safe: if writes follow (echo > file),
            // the next flush sends the actual data. If no writes follow
            // (: > file), release flushes the empty truncation.
            if data.is_empty() && matches!(mode, WriteMode::Truncate) {
                trace!(target: "nyne::fuse", ino, fh, "flush: deferring empty truncation to release");
                self.router.process_events();
                reply.ok();
                return;
            }
            match self.flush_content(ino, &data, mode) {
                Ok(()) => {
                    self.handles.clear_dirty(fh, dirty_gen);
                    self.write_errors.write().remove(&ino);
                }
                Err(e) => {
                    warn!(target: "nyne::fuse", error = %e, ino, fh, "flush failed");
                    self.write_errors.write().insert(ino, e.to_string());
                    reply.error(extract_errno(&e));
                    return;
                }
            }
        }
        self.router.process_events();
        reply.ok();
    }

    /// Releases a file handle, flushing any remaining dirty data.
    fn release(
        &self,
        req: &Request,
        ino: INodeNo,
        fh: FileHandle,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        let ino = u64::from(ino);
        let fh = u64::from(fh);
        trace!(target: "nyne::fuse", ino, fh, "release");

        let Some(entry) = self.handles.release(fh) else {
            self.router.process_events();
            reply.ok();
            return;
        };

        if entry.is_dirty() {
            if let Err(e) = self.flush_content(entry.inode, &entry.buffer, entry.write_mode()) {
                warn!(target: "nyne::fuse", inode = entry.inode, error = %e, "flush on release failed");
                self.write_errors.write().insert(ino, e.to_string());
            } else {
                self.write_errors.write().remove(&ino);
            }
        }

        // Call lifecycle release hook for virtual nodes.
        if let Some(ResolvedInode::Virtual { node, dir_path, .. }) = self.resolve_for_request(ino, req)
            && let Some(lc) = node.lifecycle()
        {
            let ctx = self.router.make_request_context(&dir_path);
            if let Err(e) = lc.release(&ctx) {
                warn!(target: "nyne::fuse", ino, error = %e, "lifecycle release failed");
            }
        }

        // Evict per-inode write lock when no handles remain for this inode.
        if !self.handles.has_handles_for_inode(ino) {
            self.write_locks.write().remove(&ino);
        }

        self.router.process_events();
        reply.ok();
    }

    /// Opens a directory for reading.
    fn opendir(&self, _req: &Request, ino: INodeNo, _flags: OpenFlags, reply: ReplyOpen) {
        let ino = u64::from(ino);
        trace!(target: "nyne::fuse", ino, "opendir");
        reply.opened(FileHandle(0), FopenFlags::empty());
    }

    /// Releases a directory handle.
    fn releasedir(&self, _req: &Request, ino: INodeNo, _fh: FileHandle, _flags: OpenFlags, reply: ReplyEmpty) {
        let ino = u64::from(ino);
        trace!(target: "nyne::fuse", ino, "releasedir");
        reply.ok();
    }

    /// Checks access permissions for an inode.
    fn access(&self, req: &Request, ino: INodeNo, mask: AccessFlags, reply: ReplyEmpty) {
        let ino = u64::from(ino);
        trace!(target: "nyne::fuse", ino, ?mask, "access");

        let resolved = self.resolve_for_request(ino, req);

        if mask.contains(AccessFlags::W_OK) {
            // Deny W_OK on real directories so editors skip atomic-save
            // (rename-to-backup → create → unlink-backup) and use direct
            // writes instead.  The actual create/rename/unlink syscalls
            // still reach FUSE handlers — without `default_permissions` the
            // kernel never gates operations on access() results.
            if let Some(ResolvedInode::Real {
                file_type: FileKind::Directory,
                ..
            }) = resolved
            {
                reply.error(Errno::EACCES);
                return;
            }

            // Deny W_OK on virtual nodes without the Writable capability.
            // Consistent with the open() rejection — tools that probe
            // access before opening get a correct answer.
            if let Some(ResolvedInode::Virtual { ref node, .. }) = resolved
                && node.writable().is_none()
            {
                reply.error(Errno::EACCES);
                return;
            }
        }

        reply.ok();
    }

    /// Reads the target of a symbolic link.
    fn readlink(&self, req: &Request, ino: INodeNo, reply: ReplyData) {
        let ino = u64::from(ino);
        debug!(target: "nyne::fuse", ino, "readlink");

        let Some(resolved) = self.resolve_for_request(ino, req) else {
            reply.error(Errno::ENOENT);
            return;
        };

        match resolved {
            ResolvedInode::Real {
                file_type: FileKind::Symlink,
                path,
            } => {
                let target = fuse_try!(
                    reply,
                    self.router.real_fs().symlink_target(&path),
                    ino,
                    "readlink failed"
                );
                reply.data(target.as_os_str().as_encoded_bytes());
            }
            ResolvedInode::Virtual { node, .. } =>
                if let NodeKind::Symlink { target } = node.kind() {
                    reply.data(target.as_os_str().as_encoded_bytes());
                } else {
                    reply.error(Errno::EINVAL);
                },
            ResolvedInode::Real { .. } => reply.error(Errno::EINVAL),
        }
    }

    /// Creates a new file in the given parent directory.
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

    /// Creates a new directory in the given parent directory.
    fn mkdir(&self, req: &Request, parent: INodeNo, name: &OsStr, _mode: u32, _umask: u32, reply: ReplyEntry) {
        self.do_mkdir(req, parent, name, reply);
    }

    /// Removes a file from the given parent directory.
    fn unlink(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEmpty) {
        self.do_remove(parent, name, false, reply);
    }

    /// Removes a directory from the given parent directory.
    fn rmdir(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEmpty) {
        self.do_remove(parent, name, true, reply);
    }

    /// Renames or moves a file or directory.
    fn rename(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        newparent: INodeNo,
        newname: &OsStr,
        _flags: RenameFlags,
        reply: ReplyEmpty,
    ) {
        self.do_rename(parent, name, newparent, newname, reply);
    }

    /// Gets an extended attribute value by name.
    fn getxattr(&self, _req: &Request, ino: INodeNo, name: &OsStr, size: u32, reply: ReplyXattr) {
        self.do_getxattr(ino, name, size, reply);
    }

    /// Lists all extended attribute names for an inode.
    fn listxattr(&self, _req: &Request, ino: INodeNo, size: u32, reply: ReplyXattr) {
        self.do_listxattr(ino, size, reply);
    }

    /// Sets an extended attribute value.
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

    /// Removes an extended attribute (not supported).
    fn removexattr(&self, _req: &Request, ino: INodeNo, name: &OsStr, reply: ReplyEmpty) {
        let ino = u64::from(ino);
        debug!(target: "nyne::fuse", ino, name = ?name, "removexattr");
        reply.error(Errno::ENOTSUP);
    }

    /// Called when the filesystem is unmounted.
    fn destroy(&mut self) {
        info!(target: "nyne::fuse", "filesystem destroyed");
    }

    /// Forgets an inode, decrementing its lookup count.
    fn forget(&self, _req: &Request, ino: INodeNo, nlookup: u64) {
        trace!(target: "nyne::fuse", ino = u64::from(ino), nlookup, "forget");
    }

    /// Creates a special file node (not supported).
    fn mknod(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        _rdev: u32,
        reply: ReplyEntry,
    ) {
        let parent = u64::from(parent);
        debug!(target: "nyne::fuse", parent, name = ?name, "mknod: not supported");
        reply.error(Errno::EPERM);
    }

    /// Creates a symbolic link (not supported).
    fn symlink(&self, _req: &Request, parent: INodeNo, link_name: &OsStr, _target: &Path, reply: ReplyEntry) {
        let parent = u64::from(parent);
        debug!(target: "nyne::fuse", parent, link_name = ?link_name, "symlink: not supported");
        reply.error(Errno::EPERM);
    }

    /// Creates a hard link (not supported).
    fn link(&self, _req: &Request, _ino: INodeNo, newparent: INodeNo, newname: &OsStr, reply: ReplyEntry) {
        let newparent = u64::from(newparent);
        debug!(target: "nyne::fuse", newparent, newname = ?newname, "link: not supported");
        reply.error(Errno::EPERM);
    }

    /// Synchronizes file contents to storage (no-op).
    fn fsync(&self, _req: &Request, ino: INodeNo, fh: FileHandle, datasync: bool, reply: ReplyEmpty) {
        trace!(target: "nyne::fuse", ino = u64::from(ino), fh = u64::from(fh), datasync, "fsync");
        reply.ok();
    }

    /// Synchronizes directory contents to storage (no-op).
    fn fsyncdir(&self, _req: &Request, ino: INodeNo, fh: FileHandle, datasync: bool, reply: ReplyEmpty) {
        trace!(target: "nyne::fuse", ino = u64::from(ino), fh = u64::from(fh), datasync, "fsyncdir");
        reply.ok();
    }

    /// Returns filesystem statistics from the underlying real filesystem.
    fn statfs(&self, _req: &Request, _ino: INodeNo, reply: ReplyStatfs) {
        match statvfs(self.router.real_fs().source_dir()) {
            Ok(st) => {
                #[expect(clippy::cast_possible_truncation, reason = "fuser API requires u32; real values fit")]
                reply.statfs(
                    st.f_blocks,
                    st.f_bfree,
                    st.f_bavail,
                    st.f_files,
                    st.f_ffree,
                    st.f_bsize as u32,
                    st.f_namemax as u32,
                    st.f_frsize as u32,
                );
            }
            Err(e) => {
                warn!(target: "nyne::fuse", error = %e, "statfs failed");
                reply.error(Errno::EIO);
            }
        }
    }

    /// Handles an ioctl request (not supported).
    fn ioctl(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _fh: FileHandle,
        _flags: IoctlFlags,
        _cmd: u32,
        _in_data: &[u8],
        _out_size: u32,
        reply: ReplyIoctl,
    ) {
        reply.error(Errno::ENOTTY);
    }

    /// Allocates space for a file (not supported).
    fn fallocate(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        _offset: u64,
        _length: u64,
        _mode: i32,
        reply: ReplyEmpty,
    ) {
        debug!(target: "nyne::fuse", ino = u64::from(ino), "fallocate: not supported");
        reply.error(Errno::ENOTSUP);
    }

    /// Seeks to a position in a file (not supported).
    fn lseek(&self, _req: &Request, _ino: INodeNo, _fh: FileHandle, _offset: i64, _whence: i32, reply: ReplyLseek) {
        reply.error(Errno::ENOTSUP);
    }

    /// Copies a range of data between files (not supported).
    fn copy_file_range(
        &self,
        _req: &Request,
        _ino_in: INodeNo,
        _fh_in: FileHandle,
        _offset_in: u64,
        _ino_out: INodeNo,
        _fh_out: FileHandle,
        _offset_out: u64,
        _len: u64,
        _flags: CopyFileRangeFlags,
        reply: ReplyWrite,
    ) {
        reply.error(Errno::ENOTSUP);
    }
}
