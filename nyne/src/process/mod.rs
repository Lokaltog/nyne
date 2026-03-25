//! Process spawning utilities.
//!
//! Provides [`Spawner`] for launching subprocesses and capturing their
//! stdio as owned file descriptors. Used by the LSP client to spawn
//! language servers as direct children of the daemon process.

use std::ffi::OsStr;
use std::fmt::Debug;
use std::os::fd::OwnedFd;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use color_eyre::eyre::{Result, WrapErr, eyre};
use parking_lot::Mutex;
use tracing::{debug, info};

/// Spawns subprocesses and owns their lifecycle.
///
/// Thread-safe: the child list is mutex-protected so multiple threads
/// (e.g., FUSE handler threads triggering LSP spawns) can call `spawn`
/// concurrently.
///
/// On drop, all children are reaped — already-exited children are
/// collected, lingering ones are killed.
pub struct Spawner {
    children: Mutex<Vec<Child>>,
}

/// Returns the default value.
impl Default for Spawner {
    /// Returns the default value.
    fn default() -> Self { Self::new() }
}

/// Process spawning and child lifecycle management.
impl Spawner {
    /// Creates a new spawner with no tracked children.
    pub const fn new() -> Self {
        Self {
            children: Mutex::new(Vec::new()),
        }
    }

    /// Spawn a subprocess and return its stdio fds.
    ///
    /// The child's environment is **cleared** — only the explicitly passed
    /// `env` pairs are set. Callers are responsible for building the
    /// desired environment (e.g., propagating specific host variables).
    pub fn spawn<A, K, V>(&self, command: &str, args: &[A], env: &[(K, V)], cwd: &Path) -> Result<SpawnedFds>
    where
        A: AsRef<OsStr> + Debug,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        info!(
            command,
            ?args,
            cwd = %cwd.display(),
            "spawning process",
        );

        let mut cmd = Command::new(command);
        cmd.args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_clear();

        for (k, v) in env {
            cmd.env(k, v);
        }

        let mut child = cmd.spawn().wrap_err_with(|| format!("spawning process: {command}"))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| eyre!("failed to capture child stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| eyre!("failed to capture child stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| eyre!("failed to capture child stderr"))?;

        let stdin_fd = OwnedFd::from(stdin);
        let stdout_fd = OwnedFd::from(stdout);
        let stderr_fd = OwnedFd::from(stderr);

        self.children.lock().push(child);

        Ok(SpawnedFds {
            stdin: stdin_fd,
            stdout: stdout_fd,
            stderr: stderr_fd,
        })
    }
}

/// Kills any lingering child processes on drop.
impl Drop for Spawner {
    /// Cleans up resources on drop.
    fn drop(&mut self) {
        let children = self.children.get_mut();
        for child in children {
            match child.try_wait() {
                Ok(Some(status)) => {
                    debug!(pid = child.id(), ?status, "child already exited");
                }
                Ok(None) => {
                    debug!(pid = child.id(), "killing lingering child");
                    let _ = child.kill();
                    let _ = child.wait();
                }
                Err(e) => {
                    debug!(pid = child.id(), error = %e, "failed to check child status");
                }
            }
        }
    }
}

/// The three stdio fds of a spawned process.
pub struct SpawnedFds {
    pub stdin: OwnedFd,
    pub stdout: OwnedFd,
    pub stderr: OwnedFd,
}
