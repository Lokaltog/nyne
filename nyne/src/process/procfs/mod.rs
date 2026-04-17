//! Helpers for reading per-process metadata from `/proc` (procfs).
//!
//! Centralizes all ad-hoc `/proc/{pid}/…` and `/proc/self/…` reads that
//! would otherwise scatter across the crate. Every caller goes through
//! this module so that:
//!
//! - Path templates live in one place — no stray `format!("/proc/{pid}/…")`
//!   calls in business logic.
//! - Kernel semantics (e.g. `TASK_COMM_LEN` truncation, trailing-newline
//!   trimming) are applied consistently.
//! - Procfs access policy (permissions, error handling) can evolve in a
//!   single spot.
//!
//! All functions return `Option<_>` — `None` means the process has exited,
//! permissions denied access, or procfs is unavailable. Callers treat
//! `None` as "no information" and never distinguish the cause.

use std::borrow::Cow;
use std::fs;
use std::os::fd::OwnedFd;

use color_eyre::eyre::{Result, WrapErr};

/// Maximum visible length of `/proc/{pid}/comm` — the kernel's
/// `TASK_COMM_LEN` minus its NUL terminator.
///
/// Process names longer than this get silently truncated by the kernel
/// when exposed via procfs, so any matching logic must truncate user
/// input to the same boundary.
pub const COMM_MAX_LEN: usize = 15;

/// Read `/proc/{pid}/comm` — the command name (executable basename).
///
/// The kernel truncates to [`COMM_MAX_LEN`] bytes. The returned value has
/// the trailing newline stripped. Returns `None` if the process has
/// exited or procfs is unavailable.
pub fn read_comm(pid: u32) -> Option<String> {
    fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|s| s.trim_end().to_owned())
}

/// Read the parent PID of `pid` from `/proc/{pid}/status`.
///
/// Parses the `PPid:` line. Returns `None` if the process has exited,
/// the `PPid:` line is missing (shouldn't happen on Linux), or the value
/// fails to parse.
pub fn read_ppid(pid: u32) -> Option<u32> {
    fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()?
        .lines()
        .find_map(|l| l.strip_prefix("PPid:").and_then(|v| v.trim().parse().ok()))
}

/// Truncate a process name to [`COMM_MAX_LEN`] bytes, matching the
/// kernel's `/proc/{pid}/comm` truncation.
///
/// Uses `floor_char_boundary` to avoid panicking on multi-byte UTF-8
/// characters at the boundary. Returns `Cow::Borrowed` when the name
/// already fits, avoiding allocation on the hot path.
pub fn truncate_comm(name: &str) -> Cow<'_, str> {
    if name.len() > COMM_MAX_LEN {
        Cow::Owned(name[..name.floor_char_boundary(COMM_MAX_LEN)].to_owned())
    } else {
        Cow::Borrowed(name)
    }
}

/// Open `/proc/self/ns/<name>` as an owned file descriptor.
///
/// Used to grab the current process's own namespace fds (e.g. `user`,
/// `mnt`) for fd-passing scenarios where the receiver cannot resolve
/// the sender's PID (e.g. attach clients in sibling PID namespaces).
/// The caller then passes the fd directly to `setns(2)`.
pub fn self_ns_fd(name: &str) -> Result<OwnedFd> {
    let path = format!("/proc/self/ns/{name}");
    Ok(fs::File::open(&path)
        .wrap_err_with(|| format!("opening {path}"))?
        .into())
}

#[cfg(test)]
mod tests;
