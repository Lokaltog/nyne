//! FUSE file attribute construction utilities.

use std::time::Duration;

use fuser::{FileType, Generation, INodeNo, Request};

use crate::router::CachePolicy;
use crate::types::Timestamps;
use crate::types::file_kind::FileKind;

/// TTL for cached attribute/entry responses on most paths.
///
/// The kernel caches `FileAttr` and directory entry results for this duration
/// before re-querying the daemon. Derived and shadow inodes override this to
/// [`Duration::ZERO`] so staleness is checked on every access.
pub(super) const TTL: Duration = Duration::from_secs(1);

/// Block size reported in `st_blksize` and used as the fallback `st_size`
/// for virtual files whose true size is unknown.
///
/// Must be non-zero: `st_size=0` causes tools like `cat` and `wc` to report
/// empty files even with `FOPEN_DIRECT_IO` (which reads until EOF).
pub(super) const BLKSIZE: u32 = 4096;

/// FUSE inode generation — always zero since we don't reuse inode numbers.
pub(super) const GENERATION: Generation = Generation(0);

/// Convert a [`FileKind`] to a FUSE [`FileType`].
///
/// Single conversion point — all FUSE file-type needs go through this.
/// Lives here to keep the `fuser` dependency out of the core trait/dispatch layers.
pub(super) const fn file_kind_to_fuse(ft: FileKind) -> FileType {
    match ft {
        FileKind::File => FileType::RegularFile,
        FileKind::Directory => FileType::Directory,
        FileKind::Symlink => FileType::Symlink,
    }
}

/// Build a `fuser::FileAttr` with common defaults.
///
/// Fills in block count, nlink (2 for directories, 1 otherwise), and
/// uid/gid from the FUSE request (so files appear owned by the caller).
/// All attr construction funnels through this function.
pub(super) fn make_attr(
    ino: u64,
    size: u64,
    kind: FileType,
    perm: u16,
    ts: Timestamps,
    req: &Request,
) -> fuser::FileAttr {
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

/// Translate a node's [`CachePolicy`] into the kernel `entry_valid` /
/// `attr_valid` TTL for FUSE replies.
///
/// SSOT for "policy → kernel TTL". Used by all attr-producing FUSE
/// paths (lookup, getattr, readdirplus, create, mkdir).
///
/// - [`CachePolicy::Default`] → `default` (caller-supplied per-file-type fallback).
/// - [`CachePolicy::NoCache`] → [`Duration::ZERO`] (kernel re-validates every access).
/// - [`CachePolicy::Ttl`] → the policy duration (kernel caches for that long).
pub(super) const fn resolve_attr_ttl(policy: CachePolicy, default: Duration) -> Duration {
    match policy {
        CachePolicy::Default => default,
        CachePolicy::NoCache => Duration::ZERO,
        CachePolicy::Ttl(d) => d,
    }
}

#[cfg(test)]
mod tests;
