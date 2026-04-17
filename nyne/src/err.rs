//! Error utilities for FUSE errno handling and eyre integration.
//!
//! Bridges two error worlds: [`rustix::io::Errno`] (used by low-level filesystem ops)
//! and [`color_eyre::eyre::Report`] (used throughout the dispatch layer). Also provides
//! [`io_err`] for constructing errors that carry an [`io::ErrorKind`], and
//! [`extract_errno`] for mapping those back to a kernel errno.

use std::io::{Error, ErrorKind};
use std::path::Path;

use color_eyre::eyre::{self, Report};
use fuser::Errno as FuseErrno;
use rustix::io::Errno;

/// Extension trait for converting `Result<T, Errno>` into `eyre::Result<T>`.
///
/// [`rustix`] returns `Result<T, Errno>` from its syscall wrappers, but the dispatch
/// layer uses `eyre::Result<T>` throughout. This trait eliminates the repetitive
/// `.map_err(|e| eyre!(e))` boilerplate at every rustix call site, replacing it
/// with a single `.into_eyre()` call.
pub trait ErrnoExt<T> {
    /// Convert the `Errno` error variant into an [`eyre::Report`].
    fn into_eyre(self) -> eyre::Result<T>;
}

/// Blanket implementation of [`ErrnoExt`] for all `Result<T, Errno>` types.
///
/// This is the only impl needed — the generic `T` covers every rustix call site
/// regardless of its success type.
impl<T> ErrnoExt<T> for Result<T, Errno> {
    /// Convert the `Errno` error into an [`eyre::Report`], discarding the typed
    /// errno in favor of eyre's dynamic error chain.
    fn into_eyre(self) -> eyre::Result<T> { self.map_err(|e| eyre::eyre!(e)) }
}

/// Create an eyre error that embeds an [`io::Error`] for FUSE errno extraction.
///
/// [`extract_errno`] walks the eyre chain to find an `io::Error` and maps its
/// `ErrorKind` to a FUSE errno. Without this wrapper, all dispatch errors surface
/// as `EIO` to userspace regardless of the actual failure reason.
///
/// Providers should use this when they need to communicate a specific errno
/// to the caller — for example, `ErrorKind::NotFound` for missing files or
/// `ErrorKind::PermissionDenied` for read-only paths. The message is for
/// logging/debugging only; FUSE clients never see it.
///
/// For the common path-based cases, prefer the typed constructors
/// ([`not_found`], [`not_readable`], [`not_writable`], [`invalid_path`],
/// [`inode_not_found`]) — they enforce a consistent message format.
pub fn io_err(kind: ErrorKind, msg: impl Into<String>) -> Report { Error::new(kind, msg.into()).into() }

/// `ErrorKind::NotFound` — target path is missing (→ `ENOENT`).
pub fn not_found(path: &Path) -> Report { io_err(ErrorKind::NotFound, format!("not found: {}", path.display())) }

/// `ErrorKind::NotFound` — unknown inode number (→ `ENOENT`).
pub fn inode_not_found(inode: u64) -> Report { io_err(ErrorKind::NotFound, format!("inode {inode} not found")) }

/// `ErrorKind::InvalidInput` — path cannot be split into (parent, name) (→ `EINVAL`).
pub fn invalid_path(path: &Path) -> Report {
    io_err(ErrorKind::InvalidInput, format!("invalid path: {}", path.display()))
}

/// `ErrorKind::PermissionDenied` — node lacks the read capability (→ `EACCES`).
pub fn not_readable(path: &Path) -> Report {
    io_err(ErrorKind::PermissionDenied, format!("not readable: {}", path.display()))
}

/// `ErrorKind::PermissionDenied` — node lacks the write capability (→ `EACCES`).
pub fn not_writable(path: &Path) -> Report {
    io_err(ErrorKind::PermissionDenied, format!("not writable: {}", path.display()))
}

/// Extract a FUSE errno from an opaque eyre error chain.
///
/// Walks the error chain looking for [`std::io::Error`]. If found, uses
/// its raw OS errno (for real I/O errors) or maps its [`ErrorKind`] (for
/// synthetic errors). Falls back to [`FuseErrno::EIO`] if no `io::Error` is
/// in the chain.
///
/// This is the SSOT for converting **dispatch-level errors** (provider
/// results, chain errors) into FUSE errnos. It exists because those
/// errors are opaque `Report`s whose errno must be discovered by walking
/// the cause chain.
///
/// Direct `FuseErrno::` constants elsewhere in the FUSE layer (e.g., returning
/// `ENOENT` when an inode is not in the map, or `ENOTSUP` for an
/// unsupported xattr operation) are **not** violations of this SSOT —
/// those are deterministic validation checks where the errno is known
/// statically at the call site and no error chain exists to inspect.
pub fn extract_errno(e: &Report) -> FuseErrno {
    for cause in e.chain() {
        if let Some(io_err) = cause.downcast_ref::<Error>() {
            if let Some(raw) = io_err.raw_os_error() {
                return FuseErrno::from_i32(raw);
            }
            return match io_err.kind() {
                ErrorKind::NotFound => FuseErrno::ENOENT,
                ErrorKind::AlreadyExists => FuseErrno::from_i32(libc::EEXIST),
                ErrorKind::PermissionDenied => FuseErrno::EACCES,
                ErrorKind::InvalidInput => FuseErrno::EINVAL,
                ErrorKind::Unsupported => FuseErrno::from_i32(libc::ENOTSUP),
                _ => FuseErrno::EIO,
            };
        }
    }
    FuseErrno::EIO
}
