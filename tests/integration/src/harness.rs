//! [`NyneMount`] — RAII fixture that spawns a real nyne FUSE daemon.
//!
//! The daemon is launched with `--storage-strategy snapshot`, which isolates
//! writes from the source repo via the built-in libgit2 snapshot cloner and
//! overlayfs. Commands execute inside the mount namespace via `nyne attach`.

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use std::{env, fs, io, process, thread};

use color_eyre::eyre::{Result, WrapErr, ensure, eyre};
use rustix::process::{Pid, Signal, kill_process, set_parent_process_death_signal};

use crate::command::CommandOutput;
use crate::git::CleanupGuard;

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(0);

const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Workspace root, resolved at runtime by walking up from the current
/// directory looking for `Cargo.toml` containing `[workspace]`.
///
/// Runtime resolution is required because test binaries may be compiled in
/// a different environment than where they run (e.g., compiled inside a
/// FUSE mount, executed against the host filesystem). Compile-time
/// `CARGO_MANIFEST_DIR` would be stale in that case.
static WORKSPACE_ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
    let cwd = env::current_dir().expect("getting current dir");
    for candidate in cwd.ancestors() {
        let manifest = candidate.join("Cargo.toml");
        if let Ok(content) = fs::read_to_string(&manifest)
            && content.contains("[workspace]")
        {
            return candidate.to_path_buf();
        }
    }
    panic!("workspace root not found from CWD: {}", cwd.display())
});

/// Absolute path to the `nyne` binary.
///
/// Resolved in priority order:
/// 1. `NYNE_BIN` environment variable (absolute path override)
/// 2. `CARGO_TARGET_DIR`/{profile}/nyne
/// 3. `<workspace>/target/{profile}/nyne`
static NYNE_BIN: LazyLock<PathBuf> = LazyLock::new(|| {
    if let Some(override_path) = env::var_os("NYNE_BIN") {
        return PathBuf::from(override_path);
    }
    let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
    let target_dir = env::var_os("CARGO_TARGET_DIR").map_or_else(|| WORKSPACE_ROOT.join("target"), PathBuf::from);
    target_dir.join(profile).join("nyne")
});

/// A running nyne FUSE mount with snapshot storage strategy.
///
/// The daemon is spawned on [`start`](Self::start) and torn down on drop.
/// All shell commands execute inside the mount's sandbox namespace via
/// `nyne attach`. Writes are captured in overlayfs — the source repo at
/// `WORKSPACE_ROOT` is never modified by tests.
pub struct NyneMount {
    /// Session ID used with `nyne attach --id <session_id>`.
    pub session_id: String,
    /// Daemon subprocess handle. `Option` so `Drop` can take ownership.
    daemon: Option<Child>,
}

impl NyneMount {
    /// Launch `nyne mount --storage-strategy snapshot` on the workspace root
    /// and wait for the daemon to accept attach requests.
    ///
    /// The spawned supervisor has `PR_SET_PDEATHSIG(SIGTERM)` set via
    /// `pre_exec`, so it receives SIGTERM the instant this test process
    /// dies — by any cause, including `std::process::exit()` which does
    /// not run destructors on static fixtures.
    pub fn start() -> Result<Self> {
        let pid = process::id();
        let counter = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
        let session_id = format!("test-{pid}-{counter}");

        let spec = format!("{session_id}:{}", WORKSPACE_ROOT.display());
        let mut cmd = Command::new(&*NYNE_BIN);
        cmd.arg("mount")
            .arg("--storage-strategy")
            .arg("snapshot")
            .arg(&spec)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        // SAFETY: set_parent_process_death_signal is an async-signal-safe
        // prctl syscall wrapper; safe to call in the post-fork/pre-exec
        // window where allocator and other unsafe operations are forbidden.
        #[allow(unsafe_code)]
        unsafe {
            cmd.pre_exec(|| set_parent_process_death_signal(Some(Signal::TERM)).map_err(io::Error::from));
        }
        let daemon = cmd.spawn().wrap_err("spawning `nyne mount`")?;

        let mut mount = Self {
            session_id,
            daemon: Some(daemon),
        };
        mount.wait_for_ready()?;
        Ok(mount)
    }

    /// Execute a shell script inside the mount namespace via `nyne attach`.
    ///
    /// The script runs with the mount root as cwd, so tests can use paths
    /// relative to the workspace root (`nyne/src/cli/mod.rs`, `@/git/...`)
    /// without worrying about how `nyne attach` maps the outer cwd.
    ///
    /// Returns captured output — does not panic on non-zero exit.
    pub fn sh(&self, script: &str) -> CommandOutput {
        // `/code` must match the default `SandboxConfig::mount_root` — the
        // single FUSE entry point inside the sandbox. Hardcoded here to avoid
        // pulling the `nyne` crate in as a test-only dependency.
        Command::new(&*NYNE_BIN)
            .arg("attach")
            .arg("--id")
            .arg(&self.session_id)
            .arg("--")
            .arg("bash")
            .arg("-c")
            .arg(format!("cd /code && {script}"))
            .output()
            .expect("invoking `nyne attach` failed to spawn")
            .into()
    }

    /// Read a VFS path via `cat`. Panics with captured stderr if the read fails.
    #[track_caller]
    pub fn read(&self, vfs_path: &str) -> String {
        let quoted = shell_quote(vfs_path);
        let out = self.sh(&format!("cat {quoted}"));
        assert!(
            out.is_ok(),
            "read failed for {vfs_path}: exit={} stderr={}",
            out.exit_code,
            out.stderr,
        );
        out.stdout
    }

    /// Create a RAII guard that restores the mount to HEAD via
    /// `git checkout HEAD -- .` on drop.
    pub const fn cleanup_guard(&self) -> CleanupGuard<'_> { CleanupGuard::new(self) }

    fn wait_for_ready(&mut self) -> Result<()> {
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        loop {
            let daemon = self.daemon.as_mut().ok_or_else(|| eyre!("daemon handle missing"))?;
            if let Some(status) = daemon.try_wait()? {
                return Err(eyre!("daemon exited before ready: {status}"));
            }
            if attach_noop(&self.session_id) {
                return Ok(());
            }
            ensure!(
                Instant::now() < deadline,
                "daemon did not become ready within {STARTUP_TIMEOUT:?}",
            );
            thread::sleep(POLL_INTERVAL);
        }
    }
}

impl Drop for NyneMount {
    fn drop(&mut self) {
        let Some(mut daemon) = self.daemon.take() else {
            return;
        };

        // Graceful shutdown via SIGTERM — the nyne daemon catches it and
        // tears down FUSE sessions cleanly before exiting. Errors are
        // ignored (ESRCH if the child already exited).
        if let Ok(raw_pid) = i32::try_from(daemon.id())
            && let Some(pid) = Pid::from_raw(raw_pid)
        {
            let _ = kill_process(pid, Signal::TERM);
        }

        let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
        while Instant::now() < deadline {
            match daemon.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => thread::sleep(POLL_INTERVAL),
                Err(_) => break,
            }
        }

        // Force kill if still alive.
        let _ = daemon.kill();
        let _ = daemon.wait();
    }
}

/// Probe the daemon with a no-op attach. Returns `true` on exit code 0.
fn attach_noop(session_id: &str) -> bool {
    Command::new(&*NYNE_BIN)
        .arg("attach")
        .arg("--id")
        .arg(session_id)
        .arg("--")
        .arg("true")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// POSIX single-quote escaping for embedding arbitrary text in shell commands.
fn shell_quote(s: &str) -> String {
    let escaped = s.replace('\'', r"'\''");
    format!("'{escaped}'")
}
