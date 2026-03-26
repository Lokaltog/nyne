//! I/O primitives for the LSP client: timeout-aware fd reading and stderr draining.
//!
//! The [`TimeoutReader`] wraps the server's stdout fd with `poll()`-based
//! timeouts so that a hung or slow server surfaces a clean `TimedOut` error
//! instead of blocking a FUSE handler thread indefinitely. Stderr is drained
//! on a separate thread via [`drain_stderr`] to prevent the server from
//! blocking on a full pipe buffer.

use std::fs::File;
use std::io::{self, BufReader, Read};
use std::os::fd::OwnedFd;
use std::time::Duration;

use rustix::event;
use rustix::event::{Nsecs, PollFlags, Timespec};
use tracing::trace;

/// A `BufRead`-compatible reader that enforces a per-read timeout via `poll()`.
///
/// Wraps an owned fd and polls it before each read. Returns
/// `io::ErrorKind::TimedOut` if the fd is not ready within the deadline,
/// which surfaces as a clean error instead of blocking the FUSE handler
/// indefinitely.
pub(super) struct TimeoutReader {
    inner: BufReader<File>,
    timeout: Duration,
}

/// Construction and poll-based timeout logic.
impl TimeoutReader {
    /// Create from an `OwnedFd` (stdout of a spawned LSP server).
    pub(super) fn from_owned_fd(fd: OwnedFd, timeout: Duration) -> Self {
        Self {
            inner: BufReader::new(File::from(fd)),
            timeout,
        }
    }

    /// Wait for the fd to become readable, up to the configured timeout.
    ///
    /// Uses `rustix::event::poll` (a thin wrapper around POSIX `ppoll`) to
    /// avoid pulling in `mio` or `tokio` for a single blocking fd. Returns
    /// `io::ErrorKind::TimedOut` on expiry so the reader thread can
    /// distinguish "server is quiet" from "server is dead".
    fn poll_ready(&self) -> io::Result<()> {
        let mut pollfd = [event::PollFd::new(self.inner.get_ref(), PollFlags::IN)];

        let timeout = Timespec {
            tv_sec: self.timeout.as_secs().cast_signed(),
            tv_nsec: Nsecs::from(self.timeout.subsec_nanos()),
        };
        let ready =
            event::poll(&mut pollfd, Some(&timeout)).map_err(|e| io::Error::from_raw_os_error(e.raw_os_error()))?;

        if ready == 0 {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("LSP server did not respond within {}s", self.timeout.as_secs()),
            ));
        }

        Ok(())
    }
}

/// `Read` impl that polls for readability before delegating to the inner buffer.
impl Read for TimeoutReader {
    /// Reads bytes, polling for readiness with a timeout when the buffer is empty.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // Only poll when the BufReader's internal buffer is empty.
        // When buffered data exists, the read completes immediately.
        if self.inner.buffer().is_empty() {
            self.poll_ready()?;
        }
        self.inner.read(buf)
    }
}

/// `BufRead` impl that polls for readability before filling the buffer.
impl io::BufRead for TimeoutReader {
    /// Fills the internal buffer, polling for readiness with a timeout when empty.
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.inner.buffer().is_empty() {
            self.poll_ready()?;
        }
        self.inner.fill_buf()
    }

    /// Marks bytes as consumed in the internal buffer.
    fn consume(&mut self, amt: usize) { self.inner.consume(amt); }
}

/// Read and log all lines from the server's stderr until EOF.
///
/// Runs on a dedicated background thread to prevent the server from blocking
/// on a full stderr pipe buffer. Lines are logged at `trace` level under the
/// `nyne::lsp` target -- visible with `RUST_LOG=nyne::lsp=trace`.
pub(super) fn drain_stderr(stderr_file: File, server_name: &str) {
    use std::io::BufRead;
    let reader = BufReader::new(stderr_file);
    for line in reader.lines() {
        match line {
            Ok(line) if !line.is_empty() => {
                trace!(target: "nyne::lsp", server = %server_name, "{line}");
            }
            Err(e) => {
                trace!(target: "nyne::lsp", server = %server_name, "stderr read error: {e}");
                break;
            }
            _ => {}
        }
    }
}
