//! Error utilities for FUSE errno handling and eyre integration.
//!
//! Bridges two error worlds: [`rustix::io::Errno`] (used by low-level filesystem ops)
//! and [`color_eyre::eyre::Report`] (used throughout the dispatch layer). Also provides
//! [`io_err`] for constructing errors that carry an [`io::ErrorKind`], which the FUSE
//! layer can extract and map to a kernel errno.

use std::io::{Error as IoError, ErrorKind};

use color_eyre::eyre::{self, Report};
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
/// The FUSE layer's [`extract_errno`](crate::fuse::extract_errno) walks the
/// eyre chain to find an `io::Error` and maps its `ErrorKind` to a FUSE
/// errno. Without this wrapper, all dispatch errors surface as `EIO` to
/// userspace regardless of the actual failure reason.
///
/// Providers should use this when they need to communicate a specific errno
/// to the caller — for example, `ErrorKind::NotFound` for missing files or
/// `ErrorKind::PermissionDenied` for read-only paths. The message is for
/// logging/debugging only; FUSE clients never see it.
pub fn io_err(kind: ErrorKind, msg: impl Into<String>) -> Report { IoError::new(kind, msg.into()).into() }
