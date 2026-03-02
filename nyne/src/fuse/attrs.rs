//! `FileAttr` construction pipeline — timestamps, permission mapping, FUSE
//! attribute building for real and virtual inodes.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fuser::{Errno, FileType, Generation, INodeNo, ReplyDirectoryPlus, Request};

use super::NyneFs;
use crate::dispatch::{ReaddirEntry, ResolvedInode, Router};
use crate::node::default_permissions::{DIR_RO, DIR_RW, FILE_RW};
use crate::node::{CachePolicy, NodeKind};
use crate::types::file_kind::FileKind;

/// TTL for cached attribute/entry responses on most paths.
const TTL: Duration = Duration::from_secs(1);

/// Block size used for `st_blksize` in file attributes and as the
/// placeholder size for virtual files.
const BLKSIZE: u32 = 4096;

/// FUSE inode generation — always zero since we don't reuse inode numbers.
pub(super) const GENERATION: Generation = Generation(0);

/// Convert a [`FileKind`] to a FUSE [`FileType`].
///
/// Single conversion point — all FUSE file-type needs go through this.
/// Lives here to keep the `fuser` dependency out of the core trait/dispatch layers.
pub const fn file_kind_to_fuse(ft: FileKind) -> FileType {
    match ft {
        FileKind::File => FileType::RegularFile,
        FileKind::Directory => FileType::Directory,
        FileKind::Symlink => FileType::Symlink,
    }
}

#[derive(Clone, Copy)]
struct Timestamps {
    atime: SystemTime,
    mtime: SystemTime,
    ctime: SystemTime,
}

/// Build a `fuser::FileAttr` with common defaults.
fn make_attr(ino: u64, size: u64, kind: FileType, perm: u16, ts: Timestamps, req: &Request) -> fuser::FileAttr {
    fuser::FileAttr {
        ino: INodeNo(ino),
        size,
        blocks: size.div_ceil(512),
        atime: ts.atime,
        mtime: ts.mtime,
        ctime: ts.ctime,
        crtime: ts.mtime,
        kind,
        perm,
        nlink: if kind == FileType::Directory { 2 } else { 1 },
        uid: req.uid(),
        gid: req.gid(),
        rdev: 0,
        blksize: BLKSIZE,
        flags: 0,
    }
}

impl NyneFs {
    /// Build a `fuser::FileAttr` and TTL for a given inode.
    ///
    /// Returns a longer TTL for gitignored / git-internal entries — they
    /// are pure passthrough with no virtual overlays, so stale dentry/attr
    /// cache is harmless.
    pub(super) fn build_attr(&self, ino: u64, req: &Request) -> Option<(fuser::FileAttr, Duration)> {
        if ino == Router::ROOT_INODE {
            return Some((Self::root_attr(req), TTL));
        }
        match self.resolve_for_request(ino, req)? {
            ResolvedInode::Real { file_type, path } => {
                let ttl = TTL;
                let meta = self.router.real_fs().metadata(&path).ok()?;
                // Directories report r-xr-xr-x to prevent editors from
                // attempting atomic saves (rename-write-unlink). Without
                // `default_permissions`, the kernel still forwards create/
                // rename/unlink to FUSE — the permission bits only affect
                // editor save-strategy decisions, not actual enforcement.
                let perm = if file_type == FileKind::Directory {
                    u16::try_from(meta.permissions & 0o7777).unwrap_or(DIR_RW) & !0o222
                } else {
                    u16::try_from(meta.permissions & 0o7777).unwrap_or(FILE_RW)
                };
                Some((
                    make_attr(
                        ino,
                        meta.size,
                        file_kind_to_fuse(file_type),
                        perm,
                        Timestamps {
                            atime: self.atime_overrides.read().get(&ino).copied().unwrap_or(UNIX_EPOCH),
                            mtime: meta.mtime,
                            ctime: meta.mtime,
                        },
                        req,
                    ),
                    ttl,
                ))
            }
            ResolvedInode::Virtual {
                node,
                dir_path,
                provider_id,
            } => {
                // Query lifecycle for custom attribute overrides.
                let ctx = self.router.make_request_context(&dir_path);
                let lifecycle_attr = node.lifecycle().and_then(|lc| lc.getattr(&ctx));

                let size = lifecycle_attr
                    .as_ref()
                    .and_then(|a| a.size)
                    .or_else(|| match node.kind() {
                        NodeKind::File { size_hint } => *size_hint,
                        _ => None,
                    })
                    .unwrap_or_else(|| match node.kind() {
                        NodeKind::File { .. } => {
                            // Read real content size through the L2 cache.
                            // Falls back to a full read pipeline on cache miss.
                            self.router
                                .find_provider(provider_id)
                                .map_or(0, |p| self.router.content_size(ino, &node, p.as_ref(), &ctx))
                        }
                        _ => 0,
                    });

                // Use UNIX_EPOCH as fallback — stability is what matters for
                // editors. SystemTime::now() caused every getattr to return a
                // different mtime, triggering "file modified since opening"
                // warnings in neovim. Providers that need real timestamps
                // should implement Lifecycle::getattr.
                let mtime = lifecycle_attr
                    .as_ref()
                    .and_then(|a| a.mtime)
                    .map_or(UNIX_EPOCH, |secs| UNIX_EPOCH + Duration::from_secs(secs));

                let ctime = lifecycle_attr
                    .as_ref()
                    .and_then(|a| a.ctime)
                    .map_or(mtime, |secs| UNIX_EPOCH + Duration::from_secs(secs));

                Some((
                    make_attr(
                        ino,
                        size,
                        file_kind_to_fuse(node.kind().file_kind()),
                        node.permissions(),
                        Timestamps {
                            atime: UNIX_EPOCH,
                            mtime,
                            ctime,
                        },
                        req,
                    ),
                    match node.cache_policy() {
                        CachePolicy::Never => Duration::ZERO,
                        CachePolicy::Cache => TTL,
                    },
                ))
            }
        }
    }

    /// Build a `FileAttr` for the root directory.
    ///
    /// Reports `r-xr-xr-x` — prevents editors from attempting atomic
    /// saves (rename-write-unlink). Actual mutations still reach FUSE
    /// handlers because `default_permissions` is not set.
    fn root_attr(req: &Request) -> fuser::FileAttr {
        let now = SystemTime::now();
        make_attr(
            Router::ROOT_INODE,
            0,
            FileType::Directory,
            DIR_RO,
            Timestamps {
                atime: now,
                mtime: now,
                ctime: now,
            },
            req,
        )
    }

    /// Build attrs for `ino` and reply, or reply `ENOENT`.
    pub(super) fn reply_attr(&self, ino: u64, req: &Request, reply: fuser::ReplyAttr) {
        if let Some((attr, ttl)) = self.build_attr(ino, req) {
            reply.attr(&ttl, &attr);
        } else {
            reply.error(Errno::ENOENT);
        }
    }

    /// Build attrs for `ino` and reply as a directory entry, or reply `ENOENT`.
    pub(super) fn reply_entry(&self, ino: u64, req: &Request, reply: fuser::ReplyEntry) {
        if let Some((attr, ttl)) = self.build_attr(ino, req) {
            reply.entry(&ttl, &attr, GENERATION);
        } else {
            reply.error(Errno::ENOENT);
        }
    }

    /// Build attrs for `entry_ino` and add a `readdirplus` entry to `reply`.
    ///
    /// Returns `true` when the reply buffer is full (caller should stop).
    /// Returns `false` if attrs couldn't be built (caller should skip).
    pub(super) fn add_dirplus_entry(
        &self,
        reply: &mut ReplyDirectoryPlus,
        entry_ino: u64,
        next_offset: u64,
        entry: &ReaddirEntry,
        req: &Request,
    ) -> bool {
        self.build_attr(entry_ino, req)
            .is_some_and(|(attr, ttl)| reply.add(INodeNo(entry_ino), next_offset, &entry.name, &ttl, &attr, GENERATION))
    }
}
