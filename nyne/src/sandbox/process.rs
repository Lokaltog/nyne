//! Process primitives: fork, pipe, exec, child lifecycle.
//!
//! Low-level building blocks used by the sandbox module's entry points
//! (`daemon_main`, `command_main`, `init_main`). Includes:
//!
//! - [`ReadyPipe`] — cross-fork readiness signaling via `pipe2(O_CLOEXEC)`
//! - [`fork_or_die`] / [`exec`] — fork+exec with error propagation
//! - [`ChildGuard`] — RAII kill-on-drop for child processes
//! - [`run_init`] — PID 1 init loop with signal forwarding and zombie reaping
//!
//! All functions assume a single-threaded context (post-fork child) unless
//! documented otherwise.
#![allow(unsafe_code)]

use std::ffi::OsString;
use std::io::Error;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::process::Command;
use std::{env, fs, process};

use color_eyre::eyre::{Result, WrapErr, eyre};
use rustix::io::{self as rstx_io, Errno};
use rustix::pipe::{PipeFlags, pipe_with};
use rustix::process::{Pid, Signal, WaitOptions, kill_process, waitpid};
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::low_level::register;
use tracing::{debug, trace, warn};

/// A pipe pair for cross-fork readiness signaling.
///
/// Uses `O_CLOEXEC` so pipe fds are automatically closed when the
/// command child calls `execvp` — only the daemon (which never execs)
/// retains them.
///
/// Fork-safe: capture raw fds via [`raw_fds`] before fork, reconstruct
/// in the child via [`from_raw`]. Call [`into_reader`] in the supervisor
/// and [`into_writer`] in the daemon child.
pub(super) struct ReadyPipe {
    read_fd: OwnedFd,
    write_fd: OwnedFd,
}

/// Construction and consumption methods for cross-fork readiness signaling.
impl ReadyPipe {
    /// Create a pipe for readiness signaling.
    pub(super) fn new() -> Result<Self> {
        let (read_fd, write_fd) = pipe_with(PipeFlags::CLOEXEC).wrap_err("pipe2()")?;
        Ok(Self { read_fd, write_fd })
    }

    /// Raw fd values for passing across the fork boundary.
    ///
    /// After `fork()`, both processes hold kernel-level copies of the
    /// same fds. Each side reconstructs a `ReadyPipe` via [`from_raw`]
    /// and then calls the appropriate `into_*` method.
    pub(super) fn raw_fds(&self) -> (RawFd, RawFd) { (self.read_fd.as_raw_fd(), self.write_fd.as_raw_fd()) }

    /// Reconstruct from raw fd values (used in child after fork).
    ///
    /// The fds must be valid open file descriptors owned by the calling
    /// process (typically after `fork()` duplicated the fd table).
    pub(super) fn from_raw(read_fd: RawFd, write_fd: RawFd) -> Self {
        // SAFETY: after fork(), the child has its own fd table with valid
        // copies of the same kernel fds captured via raw_fds().
        Self {
            read_fd: unsafe { OwnedFd::from_raw_fd(read_fd) },
            write_fd: unsafe { OwnedFd::from_raw_fd(write_fd) },
        }
    }

    /// Consume into the writer half (daemon side).
    ///
    /// Closes the read end, writes a one-byte ready signal, then closes
    /// the write end on drop.
    pub(super) fn into_writer(self) -> Result<()> {
        drop(self.read_fd);
        rstx_io::write(&self.write_fd, &[1]).wrap_err("writing ready signal")?;
        trace!("ready signal sent");
        Ok(())
    }

    /// Consume into the reader half (supervisor side).
    ///
    /// Closes the write end, blocks until the ready signal arrives, then
    /// closes the read end on drop.
    pub(super) fn into_reader(self) -> Result<()> {
        drop(self.write_fd);
        let mut buf = [0u8; 1];
        let n = rstx_io::read(&self.read_fd, &mut buf).wrap_err("reading ready signal")?;
        if n == 0 {
            return Err(eyre!("daemon closed pipe without signaling readiness (crashed?)"));
        }
        trace!("ready signal received");
        Ok(())
    }
}

/// Fork and run `child_fn` in the child process.
///
/// Returns the child's PID to the parent. The child function should diverge
/// (call `process::exit` or `exec`). If it returns, the child exits with
/// code 1.
pub(super) fn fork_or_die(child_fn: impl FnOnce()) -> Result<Pid> {
    // SAFETY: libc::fork() is the standard POSIX fork. rustix doesn't wrap
    // fork (only exposes `kernel_fork` behind the `runtime` feature), so we
    // call libc directly. The return value is checked immediately.
    let pid = unsafe { libc::fork() };
    match pid {
        -1 => Err(Error::last_os_error()).wrap_err("fork()"),
        0 => {
            // Child
            child_fn();
            process::exit(1);
        }
        child_pid => {
            // Parent — child_pid is always positive and non-zero.
            let pid = Pid::from_raw(child_pid).ok_or_else(|| eyre!("fork() returned invalid pid: {child_pid}"))?;
            Ok(pid)
        }
    }
}

/// Replace the current process image with the given command.
///
/// Uses `CommandExt::exec()` which handles `CString` conversion and
/// null termination internally — no unsafe needed.
pub(super) fn exec(command: &[OsString]) -> Result<()> {
    use std::os::unix::process::CommandExt;

    // exec() only returns if it fails — on success the process image
    // is replaced and this code never runs.
    let Some(program) = command.first() else {
        return Err(eyre!("empty command"));
    };
    let args = command.get(1..).unwrap_or(&[]);
    let err = Command::new(program).args(args).exec();
    Err(err).wrap_err("execvp")
}

/// Wait for a child process to exit and return its exit code.
///
/// Loops on `waitpid` (blocking, no `NOHANG`) to handle non-terminal
/// statuses (stopped/continued) that can occur with job control. Returns
/// the exit code directly, or `128 + signal` if the child was killed by
/// a signal (following the shell convention).
pub(super) fn wait_for_exit(pid: Pid) -> Result<i32> {
    loop {
        let result = waitpid(Some(pid), WaitOptions::empty()).wrap_err("waitpid")?;
        // With no NOHANG, waitpid blocks and always returns Some.
        let (_child, status) = result.ok_or_else(|| eyre!("waitpid returned None without NOHANG"))?;
        if let Some(code) = status.exit_status() {
            return Ok(code);
        }
        if let Some(sig) = status.terminating_signal() {
            warn!(signal = sig, "command killed by signal");
            return Ok(128 + sig);
        }
        debug!(?status, "ignoring non-terminal wait status");
    }
}

/// RAII guard that kills a child process on drop.
///
/// Ensures the child is cleaned up even if the parent panics
/// between forking and reaching explicit cleanup.
pub(super) struct ChildGuard(Option<Pid>);

/// RAII lifecycle management for a child process.
impl ChildGuard {
    /// Create a new guard for the given child pid.
    pub(super) const fn new(pid: Pid) -> Self { Self(Some(pid)) }

    /// Disarm the guard without killing the child.
    ///
    /// Use after the child has already been reaped (e.g., via `wait_for_exit`).
    pub(super) const fn defuse(&mut self) { self.0.take(); }

    /// Get the child's PID, if the guard is still armed.
    pub(super) const fn pid(&self) -> Option<Pid> { self.0 }

    /// Explicitly terminate the child and disarm the guard.
    pub(super) fn terminate(mut self) {
        if let Some(pid) = self.0.take() {
            kill_and_reap(pid);
        }
    }
}

/// Kills and reaps the child process if the guard was not defused.
///
/// This is a safety net for the case where the parent panics between
/// forking and reaching explicit cleanup (e.g., `wait_for_exit` or
/// `terminate`). Logs a warning when triggered, since drop-based cleanup
/// indicates an abnormal code path.
impl Drop for ChildGuard {
    /// Cleans up resources.
    fn drop(&mut self) {
        if let Some(pid) = self.0.take() {
            warn!(pid = pid.as_raw_pid(), "child guard triggered (parent panic?)");
            kill_and_reap(pid);
        }
    }
}

/// Send SIGTERM to a child and wait for it to exit.
///
/// If the child has already exited (`ESRCH`), returns silently — this is
/// expected when the child exits between the kill check and the actual
/// signal delivery. Used by [`ChildGuard`] drop and explicit termination.
fn kill_and_reap(pid: Pid) {
    debug!(pid = pid.as_raw_pid(), "sending SIGTERM to child");
    if let Err(e) = kill_process(pid, Signal::TERM) {
        if e != Errno::SRCH {
            warn!(error = %e, "failed to send SIGTERM to child");
        }
        return;
    }

    match waitpid(Some(pid), WaitOptions::empty()) {
        Ok(Some((_, status))) => debug!(?status, "child exited"),
        Ok(None) => debug!("waitpid returned no status"),
        Err(e) => warn!(error = %e, "failed to wait for child"),
    }
}

/// Set environment variables for the sandbox process.
///
/// SAFETY: must be called single-threaded (before any threads are
/// spawned). This is guaranteed in the PID namespace init process.
pub(super) fn set_env(key: &str, value: &str) {
    debug_assert!(
        {
            let status = fs::read_to_string("/proc/self/status").unwrap_or_default();
            status
                .lines()
                .find_map(|l| l.strip_prefix("Threads:"))
                .and_then(|v| v.trim().parse::<usize>().ok())
                == Some(1)
        },
        "set_env must be called single-threaded"
    );
    unsafe { env::set_var(key, value) };
}

/// PID 1 init process: fork the command, forward signals, and reap orphans on exit.
///
/// Runs as PID 1 in the sandbox's PID namespace. Responsibilities:
///
/// 1. Fork the user's command as a child process
/// 2. Register async-signal-safe signal forwarders for SIGINT/SIGTERM
///    (forwarded directly to the command child via `kill(2)`)
/// 3. Wait specifically for the command child (not any orphan) to exit
/// 4. Drain zombie orphans via `waitpid(None, NOHANG)` before returning
///
/// When PID 1 exits, the kernel SIGKILLs all remaining processes in the
/// namespace and reparents them to the host init for final cleanup.
///
/// Signal forwarding uses `signal_hook::low_level::register` (unsafe)
/// because `signal_hook`'s safe API installs `SA_RESTART` handlers, which
/// causes `waitpid` to restart automatically and never return `EINTR`.
pub(super) fn run_init(command: &[OsString]) -> Result<i32> {
    let command_pid = fork_or_die(|| {
        if let Err(e) = exec(command) {
            tracing::error!(error = format!("{e:?}"), "exec failed");
        }
    })
    .wrap_err("forking command from init")?;

    debug!(
        command_pid = command_pid.as_raw_pid(),
        "command forked from init (PID 1)"
    );

    // Forward SIGINT/SIGTERM directly to the command child from the
    // signal handler. This avoids the SA_RESTART problem: signal_hook
    // installs handlers with SA_RESTART, so waitpid is automatically
    // restarted and EINTR never reaches our code. By forwarding in
    // the handler itself, signals are delivered immediately.
    //
    // SAFETY: kill is async-signal-safe. Pid is Copy, no heap allocation.
    let cmd_pid = command_pid;
    unsafe {
        register(SIGINT, move || {
            let _ = kill_process(cmd_pid, Signal::INT);
        })
        .wrap_err("registering SIGINT forwarder in init")?;
        register(SIGTERM, move || {
            let _ = kill_process(cmd_pid, Signal::TERM);
        })
        .wrap_err("registering SIGTERM forwarder in init")?;
    }

    // Wait specifically for the command child. Using Some(command_pid)
    // instead of None prevents accidentally reaping an orphaned process
    // (e.g., fish's background helpers) and missing the command exit.
    let exit_code = loop {
        match waitpid(Some(command_pid), WaitOptions::empty()) {
            Ok(Some((_, status))) => {
                if let Some(code) = status.exit_status() {
                    debug!(code, "command exited");
                    break code;
                }
                if let Some(sig) = status.terminating_signal() {
                    warn!(signal = sig, "command killed by signal");
                    break 128 + sig;
                }
                // Non-terminal status (stopped/continued) — keep waiting.
            }
            Ok(None) => {}                   // Shouldn't happen without NOHANG.
            Err(e) if e == Errno::INTR => {} // Signal interrupted (if SA_RESTART wasn't set).
            Err(e) if e == Errno::CHILD => {
                warn!("command child disappeared unexpectedly");
                break 1;
            }
            Err(e) => return Err(e).wrap_err("waitpid in init"),
        }
    };

    // Drain orphaned zombies before exiting. Processes still running
    // (not yet zombies) are not reaped here — when PID 1 exits, the
    // kernel SIGKILLs all remaining processes in the namespace and
    // reparents them to the host init for final cleanup.
    while let Ok(Some(_)) = waitpid(None, WaitOptions::NOHANG) {}

    Ok(exit_code)
}
