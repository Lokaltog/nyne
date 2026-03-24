use std::io::{Error as IoError, ErrorKind};

/// Extension trait for converting [`rustix::io::Errno`] into [`color_eyre::eyre::Report`].
///
/// Eliminates the `.map_err(|e| eyre!(e))` boilerplate at every rustix call site.
use color_eyre::eyre::{self, Report};
use rustix::io::Errno;

/// Extension trait for converting `Result<T, Errno>` into `eyre::Result<T>`.
pub trait ErrnoExt<T> {
    fn into_eyre(self) -> eyre::Result<T>;
}

/// Blanket implementation of [`ErrnoExt`] for `Result<T, Errno>`.
impl<T> ErrnoExt<T> for Result<T, Errno> {
    /// Convert an `Errno` error into an `eyre::Report`.
    fn into_eyre(self) -> eyre::Result<T> { self.map_err(|e| eyre::eyre!(e)) }
}

/// Create an eyre error that embeds an [`io::Error`] for FUSE errno extraction.
///
/// The FUSE layer's [`extract_errno`](crate::fuse::extract_errno) walks the
/// eyre chain to find an `io::Error` and maps its `ErrorKind` to a FUSE
/// errno. Without this, all dispatch errors surface as `EIO`.
pub fn io_err(kind: ErrorKind, msg: impl Into<String>) -> Report { IoError::new(kind, msg.into()).into() }
