// Pure I/O utilities for the LSP client: timeout-aware fd reading and stderr draining.

use std::fs::File;
use std::io::{self, BufReader, Read};
use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd, RawFd};
use std::time::Duration;

use rustix::event::{Nsecs, PollFlags, Timespec};
use rustix::{event, io as rstx_io};
use tracing::trace;

/// A `BufRead`-compatible reader that enforces a per-read timeout via `poll()`.
///
/// Wraps an owned fd and polls it before each read. Returns
/// `io::ErrorKind::TimedOut` if the fd is not ready within the deadline,
/// which surfaces as a clean error instead of blocking the FUSE handler
/// indefinitely.
///
/// The `OwnedFd` is stored separately from the `BufReader` so we can
/// borrow the fd safely for poll without `unsafe`.
pub(super) struct TimeoutReader {
    /// Owned stdout handle — kept alive so the fd remains valid for poll.
    stdout_fd: OwnedFd,
    /// Buffered reader wrapping a `Read` adapter over the stdout fd.
    inner: BufReader<PollRead>,
    timeout: Duration,
}

/// Thin `Read` adapter that reads from a raw fd.
///
/// Exists solely so `BufReader` can wrap something while the actual
/// `ChildStdout` is held separately for fd borrowing in `poll()`.
struct PollRead {
    fd: RawFd,
}

impl Read for PollRead {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // read via rustix — no unsafe needed.
        let n = rstx_io::read(
            // SAFETY (of borrow_raw): fd is valid because TimeoutReader owns
            // the ChildStdout that backs it, and PollRead only exists inside
            // TimeoutReader.
            #[allow(unsafe_code)]
            unsafe {
                BorrowedFd::borrow_raw(self.fd)
            },
            buf,
        )
        .map_err(|e| io::Error::from_raw_os_error(e.raw_os_error()))?;
        Ok(n)
    }
}

impl TimeoutReader {
    /// Create from an `OwnedFd` (stdout of a spawned LSP server).
    pub(super) fn from_owned_fd(fd: OwnedFd, timeout: Duration) -> Self {
        let raw = fd.as_raw_fd();
        Self {
            stdout_fd: fd,
            inner: BufReader::new(PollRead { fd: raw }),
            timeout,
        }
    }

    /// Wait for the fd to become readable, up to the configured timeout.
    fn poll_ready(&self) -> io::Result<()> {
        let mut pollfd = [event::PollFd::new(&self.stdout_fd, PollFlags::IN)];

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

impl Read for TimeoutReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // Only poll when the BufReader's internal buffer is empty.
        // When buffered data exists, the read completes immediately.
        if self.inner.buffer().is_empty() {
            self.poll_ready()?;
        }
        self.inner.read(buf)
    }
}

impl io::BufRead for TimeoutReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.inner.buffer().is_empty() {
            self.poll_ready()?;
        }
        self.inner.fill_buf()
    }

    fn consume(&mut self, amt: usize) { self.inner.consume(amt); }
}

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
