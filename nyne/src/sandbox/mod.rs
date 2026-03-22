//! Sandboxed FUSE mount via Linux namespaces.
//!
//! Encapsulates namespace/fork/mount primitives behind safe Rust APIs.
//!
//! Two entry points:
//! - [`run_mount`] — start a FUSE daemon (no command, blocks until signal)
//! - [`run_attach`] — attach to a running daemon and exec a command
//!
//! The [`mnt`] submodule is `pub(crate)` — it's the SSOT for all mount
//! syscalls.

/// Evaluate a rustix syscall, converting its error via `ErrnoExt::into_eyre`
/// and attaching a formatted context message. Propagates with `?`.
///
/// ```ignore
/// syscall_try!(mount("tmpfs", target, ...), "mounting tmpfs at {}", target.display());
/// ```
macro_rules! syscall_try {
    ($expr:expr, $($args:tt)+) => {
        $expr.into_eyre().wrap_err_with(|| format!($($args)+))?
    };
}

mod clone;
pub mod control;
pub mod mnt;
mod namespace;
mod overlay;
pub mod paths;
mod process;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use color_eyre::eyre::{Result, WrapErr, ensure, eyre};
use rustix::process::{Pid, getgid, getpid, getuid};
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::iterator::Signals;
use tracing::{debug, info};

use self::namespace::Namespace;
use self::process::{ChildGuard, ReadyPipe, fork_or_die, wait_for_exit};
use crate::session;

/// A mount path entry for FUSE + overlay mounting.
pub struct MountEntry {
    /// Absolute path to the directory to mount.
    pub path: PathBuf,
}

/// Real host uid/gid captured before namespace creation.
///
/// Passed to the command child so it can remap back to the real user
/// identity after mount operations are complete.
#[derive(Clone, Copy)]
struct HostIdentity {
    uid: u32,
    gid: u32,
}

/// Configuration for mounting a FUSE daemon (no command, no sandbox).
pub struct MountConfig {
    /// Mount path (single path for now).
    pub mount: MountEntry,
}

impl MountConfig {
    /// Create a new mount configuration.
    ///
    /// The mount path must be an absolute directory.
    pub fn new(mount: MountEntry) -> Result<Self> {
        ensure!(
            mount.path.is_absolute(),
            "path must be absolute: {}",
            mount.path.display()
        );
        ensure!(mount.path.is_dir(), "not a directory: {}", mount.path.display());
        Ok(Self { mount })
    }
}

pub struct AttachConfig {
    /// Daemon PID to attach to.
    pub daemon_pid: i32,
    /// Mount path the daemon serves.
    pub mount_path: PathBuf,
    /// Control socket path (for injecting `NYNE_CONTROL_SOCKET` env var).
    pub control_socket: Option<PathBuf>,
    /// Command to execute inside the namespace.
    pub command: Vec<OsString>,
    /// Sandbox configuration (hostname, bind mounts, etc.).
    pub sandbox: SandboxConfig,
}

/// Callback that mounts FUSE for a given path and returns a session guard.
///
/// The returned `Box<dyn Send>` owns the [`fuser::BackgroundSession`] (and
/// any associated resources). When dropped, it triggers unmount. The sandbox
/// holds these guards for the lifetime of the daemon.
pub type MountFn = Box<dyn FnMut(&Path) -> Result<Box<dyn Send>> + Send>;

use std::env;
use std::path::Path;
use std::process::exit;
use std::sync::atomic::AtomicBool;

pub use overlay::{prepare_project_storage, resolve_persist_root};
/// The fixed mount point for the FUSE filesystem inside the sandbox.
///
/// All project access goes through this path. Used by `cli/mount.rs` to
/// set the display root for provider templates and LSP path rewriting.
pub use paths::SANDBOX_CODE;
use signal_hook::flag::register_conditional_default;

use crate::config::SandboxConfig;

/// Owns a running FUSE daemon and its session file.
struct MountSession {
    daemon: ChildGuard,
    session_path: PathBuf,
    session_id: session::SessionId,
}

/// Fork a single FUSE daemon, wait for readiness, and write its session file.
///
/// Returns without blocking — the daemon runs in a child process.
fn start_one(config: MountConfig, session_id: &session::SessionId, mount_fn: MountFn) -> Result<MountSession> {
    let pipe = ReadyPipe::new().wrap_err("creating readiness pipe")?;
    let path = config.mount.path.clone();

    info!(path = %path.display(), id = %session_id, "launching FUSE daemon");

    let mounts = [config.mount];
    let fds = pipe.raw_fds();
    let daemon_pid = fork_or_die(|| {
        let child_pipe = ReadyPipe::from_raw(fds.0, fds.1);
        daemon_main(&mounts, child_pipe, mount_fn);
    })
    .wrap_err("forking FUSE daemon")?;

    let daemon = ChildGuard::new(daemon_pid);

    info!(daemon_pid = daemon_pid.as_raw_pid(), "FUSE daemon forked");

    // Wait for daemon readiness (consumes pipe, closes fds in parent).
    pipe.into_reader().wrap_err("waiting for daemon readiness")?;
    info!("FUSE daemon ready, mount active");

    // Write session file so `attach` can discover this daemon.
    let session_path = session::write(session_id, &path, daemon_pid.as_raw_pid()).wrap_err("writing session file")?;

    Ok(MountSession {
        daemon,
        session_path,
        session_id: session_id.clone(),
    })
}

/// Block until SIGINT/SIGTERM, then tear down all sessions.
fn wait_and_shutdown(sessions: Vec<MountSession>) -> Result<()> {
    for s in &sessions {
        info!(
            "daemon running — use `nyne attach {} -- <command>` to enter",
            s.session_id
        );
    }

    let mut signals = Signals::new([SIGINT, SIGTERM]).wrap_err("registering signal handler")?;
    if let Some(sig) = signals.forever().next() {
        info!(signal = sig, "received shutdown signal");
    }

    teardown(sessions);
    Ok(())
}

/// Remove session files and terminate daemons.
fn teardown(sessions: Vec<MountSession>) {
    for s in sessions {
        session::remove(&s.session_path);
        s.daemon.terminate();
        info!(id = %s.session_id, "daemon terminated");
    }
}

/// Mount one or more FUSE daemons, write session files, and block until signal.
///
/// Each mount is forked as a separate child process. Once all daemons are
/// ready, the supervisor blocks until SIGINT/SIGTERM, then tears down all
/// sessions. If any mount fails to start, already-running daemons are
/// cleaned up before the error is propagated.
pub fn run_mounts(mounts: Vec<(MountConfig, session::SessionId, MountFn)>) -> Result<()> {
    let mut sessions = Vec::with_capacity(mounts.len());

    for (config, id, mount_fn) in mounts {
        match start_one(config, &id, mount_fn) {
            Ok(session) => sessions.push(session),
            Err(e) => {
                teardown(sessions);
                return Err(e);
            }
        }
    }

    wait_and_shutdown(sessions)
}

pub fn run_attach(config: AttachConfig) -> Result<i32> {
    let daemon_pid =
        Pid::from_raw(config.daemon_pid).ok_or_else(|| eyre!("invalid daemon PID: {}", config.daemon_pid))?;

    info!(
        daemon_pid = config.daemon_pid,
        path = %config.mount_path.display(),
        command = ?config.command,
        "attaching to daemon"
    );

    let ns = Namespace::open_from_pid(daemon_pid).wrap_err("opening daemon namespace fds")?;

    // Capture real identity before namespace entry.
    let identity = HostIdentity {
        uid: getuid().as_raw(),
        gid: getgid().as_raw(),
    };

    let cwd = env::current_dir().wrap_err("getting current directory")?;

    let fuse_path = config.mount_path.clone();
    let control_socket = config.control_socket;
    let command = config.command;
    let sandbox = config.sandbox;
    let command_pid = fork_or_die(|| {
        let paths = CommandPaths {
            cwd: &cwd,
            fuse_path: &fuse_path,
            control_socket: control_socket.as_deref(),
        };
        command_main(ns, &paths, &command, &sandbox, identity);
    })
    .wrap_err("forking command process")?;

    // RAII guard: ensures command child is killed if we panic before
    // reaching wait_for_exit (e.g., signal handler registration fails).
    let mut guard = ChildGuard::new(command_pid);

    info!(command_pid = command_pid.as_raw_pid(), "command process forked");

    // Suppress SIGINT in parent so Ctrl+C goes to the command child
    // (same process group) without killing us before cleanup.
    register_conditional_default(SIGINT, Arc::new(AtomicBool::new(false))).wrap_err("suppressing SIGINT")?;

    let exit_code = wait_for_exit(command_pid).wrap_err("waiting for command process")?;
    guard.defuse();
    info!(exit_code, "command exited");

    Ok(exit_code)
}

/// FUSE daemon entry point (runs in child 1).
///
/// Creates a user+mount namespace, mounts FUSE over each path,
/// signals readiness, then blocks for shutdown.
fn daemon_main(mounts: &[MountEntry], pipe: ReadyPipe, mut mount_fn: MountFn) {
    let run = || -> Result<()> {
        namespace::unshare_user_mount()?;

        mnt::private()?;
        debug!("mount propagation disabled");

        // Mount FUSE over each path.
        let mut sessions: Vec<Box<dyn Send>> = Vec::with_capacity(mounts.len());
        for entry in mounts {
            info!(path = %entry.path.display(), "mounting FUSE");
            let session =
                mount_fn(&entry.path).wrap_err_with(|| format!("mounting FUSE at {}", entry.path.display()))?;
            info!(path = %entry.path.display(), "FUSE mount active");
            sessions.push(session);
        }

        // Signal readiness to supervisor (consumes pipe, closes fds).
        pipe.into_writer()?;
        debug!("readiness signaled, serving FUSE");

        // Block until SIGINT or SIGTERM. All processes share a process group,
        // so Ctrl+C delivers SIGINT to the daemon too — handle both for
        // clean FUSE unmount via BackgroundSession::drop.
        let mut signals = Signals::new([SIGINT, SIGTERM]).wrap_err("registering daemon signal handler")?;
        if let Some(sig) = signals.forever().next() {
            info!(signal = sig, "daemon received shutdown signal");
        }

        // Sessions dropped here — triggers BackgroundSession::drop
        // which calls umount_and_join on each FUSE mount.
        drop(sessions);
        debug!("FUSE sessions unmounted, daemon exiting");

        // Notify user about cached clone data that can be cleaned up.
        if let Some(base_dirs) = directories::BaseDirs::new() {
            let pid = getpid();
            let lower = paths::lower_base(base_dirs.cache_dir(), pid);
            let merged = paths::merged_base(pid);
            info!(
                "session ended — cached data can be removed with:\n  rm -rf {} {}",
                lower.display(),
                merged.display(),
            );
        }

        Ok(())
    };

    if let Err(e) = run() {
        tracing::error!(error = format!("{e:?}"), "daemon failed");
        exit(1);
    }
    exit(0);
}

/// Remap host CWD to sandbox path.
///
/// If the CWD is inside the FUSE-mounted project, remap to `/code/...`.
/// Otherwise return the original CWD (it may exist via RO bind mounts).
fn sandbox_cwd(cwd: &Path, fuse_path: &Path) -> PathBuf {
    if let Ok(relative) = cwd.strip_prefix(fuse_path) {
        PathBuf::from(paths::SANDBOX_CODE).join(relative)
    } else {
        cwd.to_path_buf()
    }
}

/// Bundled paths for the command child process.
struct CommandPaths<'a> {
    cwd: &'a Path,
    fuse_path: &'a Path,
    control_socket: Option<&'a Path>,
}

fn command_main(
    ns: Namespace,
    paths: &CommandPaths<'_>,
    command: &[OsString],
    sandbox: &SandboxConfig,
    identity: HostIdentity,
) {
    let run = || -> Result<()> {
        ns.enter()?;
        debug!("entered daemon namespace");

        overlay::setup(paths.fuse_path, &sandbox.bind_mounts)?;

        // Set hostname before dropping root — NEWUTS requires CAP_SYS_ADMIN.
        namespace::unshare_uts(&sandbox.hostname)?;

        // PID namespace: the next fork creates PID 1.
        namespace::unshare_pid()?;

        let command = command.to_vec();
        let cwd = paths.cwd.to_path_buf();
        let fuse_path = paths.fuse_path.to_path_buf();
        let mut sandbox_env = sandbox.env.clone();

        // Inject control socket path so `nyne exec` works inside the sandbox.
        if let Some(socket_path) = paths.control_socket
            && let Some(s) = socket_path.to_str()
        {
            sandbox_env
                .entry(control::NYNE_CONTROL_SOCKET_ENV.to_owned())
                .or_insert_with(|| s.to_owned());
        }
        let init_pid = fork_or_die(|| {
            init_main(&command, identity, &cwd, &fuse_path, &sandbox_env);
        })
        .wrap_err("forking init (PID 1)")?;

        let exit_code = wait_for_exit(init_pid).wrap_err("waiting for init")?;
        exit(exit_code);
    };

    if let Err(e) = run() {
        tracing::error!(error = format!("{e:?}"), command = ?command, "command failed");
        exit(1);
    }
}

/// Init process entry point (PID 1 in the new PID namespace).
///
/// Runs in the forked child after `unshare(CLONE_NEWPID)`. Remounts
/// `/proc` while still root in the daemon's user namespace, drops to
/// the real user identity, sets the working directory, then delegates
/// to [`process::run_init`] for the fork-exec-reap loop.
fn init_main(
    command: &[OsString],
    identity: HostIdentity,
    cwd: &Path,
    fuse_path: &Path,
    extra_env: &HashMap<String, String>,
) {
    let run = || -> Result<()> {
        // Remount /proc for the new PID namespace. Must happen before
        // user remap — mounting requires CAP_SYS_ADMIN in the user
        // namespace that owns the mount namespace.
        mnt::proc(Path::new("/proc"))?;
        debug!("/proc remounted for PID namespace");

        // Drop root identity.
        namespace::unshare_user_remap(identity.uid, identity.gid)?;

        // Set CWD: remap to /code if inside the FUSE-mounted project,
        // fall back to /code if the target doesn't exist in the sandbox.
        let target_cwd = sandbox_cwd(cwd, fuse_path);
        if env::set_current_dir(&target_cwd).is_err() {
            env::set_current_dir(paths::SANDBOX_CODE).wrap_err("setting working directory to /code fallback")?;
        }
        debug!(cwd = %env::current_dir().unwrap_or_default().display(), "working directory set");

        for (key, value) in extra_env {
            process::set_env(key, value);
        }

        // Prepend the nyne bin directory so the sandbox resolves the same
        // binary that was invoked, regardless of what's installed on PATH.
        let nyne_path = match env::var("PATH") {
            Ok(existing) => format!("{}:{existing}", paths::NYNE_BIN_DIR),
            Err(_) => paths::NYNE_BIN_DIR.to_owned(),
        };
        process::set_env("PATH", &nyne_path);

        let exit_code = process::run_init(command)?;
        exit(exit_code);
    };

    if let Err(e) = run() {
        tracing::error!(error = format!("{e:?}"), "init failed");
        exit(1);
    }
}
