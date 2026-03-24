// Pure I/O utilities for the LSP client: timeout-aware fd reading and stderr draining.

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

impl TimeoutReader {
    /// Create from an `OwnedFd` (stdout of a spawned LSP server).
    pub(super) fn from_owned_fd(fd: OwnedFd, timeout: Duration) -> Self {
        Self {
            inner: BufReader::new(File::from(fd)),
            timeout,
        }
    }

    /// Wait for the fd to become readable, up to the configured timeout.
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
