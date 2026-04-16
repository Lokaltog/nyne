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

/// Reap overlay/pivot scaffolding dirs left behind by terminated processes.
mod cleanup;
/// Control socket IPC between daemon and CLI commands.
pub mod control;
/// Mount syscall primitives for sandbox construction.
pub mod mnt;
/// Linux namespace creation and entry (user, mount, PID, UTS).
mod namespace;
/// Project storage and sandbox filesystem isolation via overlay and pivot_root.
mod overlay;
/// Well-known path constructors for sandbox operations.
pub mod paths;
/// Process primitives: fork, pipe, exec, and child lifecycle management.
mod process;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use color_eyre::eyre::{Result, WrapErr, ensure};
use linkme::distributed_slice;
use rustix::process::{getgid, getuid};
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::iterator::Signals;
use tracing::{debug, info, warn};

pub use self::namespace::Namespace;
use self::process::{ChildGuard, ReadyPipe, fork_or_die, wait_for_exit};
use crate::config::StorageStrategy;
use crate::session;

/// A mount path entry for FUSE + overlay mounting.
///
/// Each entry represents a single project directory that the daemon will
/// serve via FUSE. Currently one mount per daemon, but the `[MountEntry]`
/// slice pattern in internal APIs allows future multi-mount expansion
/// without API changes.
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
///
/// Used by `run_mounts` to fork and start FUSE daemons. The daemon runs
/// in its own user+mount namespace and serves FUSE until a shutdown signal
/// arrives. Sandbox isolation (`pivot_root`, bind mounts) is handled
/// separately by the command child via [`AttachConfig`].
pub struct DaemonConfig {
    /// Mount path (single path for now).
    pub mount: MountEntry,
}

/// Construction and validation for [`DaemonConfig`].
impl DaemonConfig {
    /// Create a new mount configuration.
    ///
    /// Validates that the mount path is an absolute directory. Returns an
    /// error if the path is relative or does not point to an existing directory.
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

/// Configuration for attaching to a running nyne daemon's namespace.
///
/// Passed to [`run_attach`], which forks a command child that enters the
/// daemon's user+mount namespace, builds an isolated root via `pivot_root`,
/// and execs the user's command inside the sandbox. The sandbox configuration
/// (hostname, bind mounts, env vars) is carried here so the command child
/// has everything it needs without additional IPC.
pub struct AttachConfig {
    /// Mount path the daemon serves.
    pub mount_path: PathBuf,
    /// Control socket path. The daemon's user + mount namespace fds are
    /// fetched from this socket via `SCM_RIGHTS` (see
    /// [`control::recv_namespace_fds`]), and the path is injected as
    /// `NYNE_CONTROL_SOCKET`.
    pub control_socket: PathBuf,
    /// Nested session directory for inside-sandbox `nyne` invocations
    /// (for injecting `NYNE_SESSION_DIR` env var).
    pub session_dir: PathBuf,
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
/// Trait for cloning a project directory for use as an overlay lowerdir.
///
/// Implemented by the git plugin to provide snapshot and hardlink cloning
/// strategies. Core discovers implementations at link time via the
/// [`PROJECT_CLONERS`] distributed slice.
pub trait ProjectCloner: Send + Sync {
    /// Clone `source` into `target` using the given storage strategy.
    fn clone_project(&self, source: &Path, target: &Path, strategy: StorageStrategy) -> Result<()>;
}

/// Factory function type for project cloners.
pub type ClonerFactory = fn() -> Box<dyn ProjectCloner>;

/// Link-time distributed slice of project cloner factories.
///
/// Plugin crates contribute entries via `#[distributed_slice(PROJECT_CLONERS)]`.
/// At mount time, `prepare_project_storage` picks the first available cloner.
#[allow(unsafe_code)]
#[distributed_slice]
pub static PROJECT_CLONERS: [ClonerFactory];

use std::env;
use std::path::Path;
use std::process::exit;
use std::sync::atomic::AtomicBool;

pub use overlay::prepare_project_storage;
use signal_hook::flag::register_conditional_default;

use crate::config::SandboxConfig;

/// Owns a running FUSE daemon and its session file.
///
/// Created by [`start_one`] after the daemon signals readiness. The
/// [`ChildGuard`] ensures the daemon child is killed on drop (e.g., if
/// a subsequent mount fails during multi-mount startup). The session file
/// is written to `$XDG_RUNTIME_DIR/nyne/` so `nyne attach` can discover
/// running daemons by session ID.
struct MountSession {
    daemon: ChildGuard,
    session_path: PathBuf,
    session_id: session::SessionId,
}

/// Fork a single FUSE daemon, wait for readiness, and write its session file.
///
/// Returns without blocking — the daemon runs in a child process.
fn start_one(config: DaemonConfig, session_id: &session::SessionId, mount_fn: MountFn) -> Result<MountSession> {
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
fn wait_and_shutdown(sessions: Vec<MountSession>, state_root: &Path) -> Result<()> {
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

    teardown(sessions, state_root);
    Ok(())
}

/// Remove session files, terminate daemons, and reap their state trees.
///
/// After each daemon is killed and reaped, its mount namespace is gone
/// and the per-PID state tree `<state_root>/proc/<pid>` on the host is
/// no longer a mount hierarchy — `ProcState::reap` removes it safely.
fn teardown(sessions: Vec<MountSession>, state_root: &Path) {
    for s in sessions {
        session::remove(&s.session_path);
        let daemon_pid = s.daemon.pid();
        let session_id = s.session_id.clone();
        s.daemon.terminate();
        if let Some(pid) = daemon_pid {
            paths::ProcState::new(state_root, pid).reap();
        }
        info!(id = %session_id, "daemon terminated");
    }
}

/// Mount one or more FUSE daemons, write session files, and block until signal.
///
/// Each mount is forked as a separate child process. Once all daemons are
/// ready, the supervisor blocks until SIGINT/SIGTERM, then tears down all
/// sessions. If any mount fails to start, already-running daemons are
/// cleaned up before the error is propagated.
pub fn run_mounts(mounts: Vec<(DaemonConfig, session::SessionId, MountFn)>, state_root: &Path) -> Result<()> {
    // Clean up any per-process state trees left behind by crashed
    // predecessors before spawning our own daemons.
    cleanup::reap_stale(state_root);

    let mut sessions = Vec::with_capacity(mounts.len());

    for (config, id, mount_fn) in mounts {
        match start_one(config, &id, mount_fn) {
            Ok(session) => sessions.push(session),
            Err(e) => {
                teardown(sessions, state_root);
                return Err(e);
            }
        }
    }

    wait_and_shutdown(sessions, state_root)
}

/// Attach to a running daemon and execute a command inside its namespace.
///
/// Forks a command child that enters the daemon's user+mount namespace,
/// builds an isolated root filesystem via `pivot_root`, creates PID and
/// UTS namespaces, then execs the user's command as PID 1's child. The
/// parent suppresses SIGINT (so Ctrl+C goes to the command child's process
/// group) and waits for the child to exit.
///
/// Returns the command's exit code (0-255), or 128+signal if killed.
pub fn run_attach(config: AttachConfig) -> Result<i32> {
    // Fetch the daemon's user + mount namespace fds directly via
    // SCM_RIGHTS on the control socket. This bypasses PID resolution
    // entirely: fd passing works across unrelated PID namespaces
    // (e.g. two parallel `nyne attach` shells running sibling init
    // PID namespaces), whereas SO_PEERCRED and `/proc/<pid>/ns/*`
    // lookups do not.
    let ns = control::recv_namespace_fds(&config.control_socket).wrap_err("requesting daemon namespace fds")?;

    info!(
        path = %config.mount_path.display(),
        command = ?config.command,
        "attaching to daemon"
    );

    // Capture real identity before namespace entry.
    let identity = HostIdentity {
        uid: getuid().as_raw(),
        gid: getgid().as_raw(),
    };

    let cwd = env::current_dir().wrap_err("getting current directory")?;

    let fuse_path = config.mount_path.clone();
    let control_socket = config.control_socket;
    let session_dir = config.session_dir;
    let command = config.command;
    let sandbox = config.sandbox;
    let state_root = sandbox.state_root.clone();
    let mount_root = sandbox.mount_root.clone();
    let command_pid = fork_or_die(|| {
        let paths = CommandPaths {
            cwd: &cwd,
            fuse_path: &fuse_path,
            mount_root: &mount_root,
            control_socket: &control_socket,
            session_dir: &session_dir,
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
    // The command child's mount namespace is gone now; its per-PID state
    // tree `<state_root>/proc/<pid>` is just empty dirs on the host.
    paths::ProcState::new(&state_root, command_pid).reap();
    info!(exit_code, "command exited");

    Ok(exit_code)
}

/// Run a fallible closure, exiting the process on success (0) or failure (1).
///
/// Used by sandbox entry points (daemon, command, init) that run in forked
/// child processes and must terminate via `exit()`.
fn run_or_exit(label: &str, f: impl FnOnce() -> Result<()>) -> ! {
    if let Err(e) = f() {
        tracing::error!(process = label, error = format!("{e:?}"), "process failed");
        exit(1);
    }
    exit(0);
}
/// FUSE daemon entry point (runs in child 1).
///
/// Creates a user+mount namespace, mounts FUSE over each path,
/// signals readiness, then blocks for shutdown.
fn daemon_main(mounts: &[MountEntry], pipe: ReadyPipe, mut mount_fn: MountFn) {
    run_or_exit("daemon", || {
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
        // which calls umount_and_join on each FUSE mount. Per-PID
        // scaffolding directories are reaped by the supervisor in
        // `teardown` once this process is waitpid-reaped.
        drop(sessions);
        debug!("FUSE sessions unmounted, daemon exiting");

        Ok(())
    });
}

/// Remap host CWD to sandbox path.
///
/// If the CWD is inside the FUSE-mounted project, remap to `mount_root/...`.
/// Otherwise return the original CWD (it may exist via RO bind mounts).
fn sandbox_cwd(cwd: &Path, fuse_path: &Path, mount_root: &Path) -> PathBuf {
    if let Ok(relative) = cwd.strip_prefix(fuse_path) {
        mount_root.join(relative)
    } else {
        cwd.to_path_buf()
    }
}

/// Bundled paths for the command child process.
///
/// Groups the host-side paths needed by [`command_main`] and [`init_main`]
/// to set up the sandbox: the original working directory (for CWD
/// remapping), the FUSE mount path (for overlay setup and `pivot_root`),
/// the sandbox mount root (for CWD fallback and env setup), the control
/// socket path (injected as `NYNE_CONTROL_SOCKET`), and the nested
/// session dir (injected as `NYNE_SESSION_DIR`).
struct CommandPaths<'a> {
    cwd: &'a Path,
    fuse_path: &'a Path,
    mount_root: &'a Path,
    control_socket: &'a Path,
    session_dir: &'a Path,
}

/// Insert a path as an env var entry, UTF-8 path permitting.
///
/// Only inserts if the key is not already set (so explicit user config
/// wins). Warns and skips when the path is not UTF-8 — env values must
/// be strings, and silently skipping would hide the missing var inside
/// the sandbox.
fn inject_path_env(env: &mut HashMap<String, String>, key: &str, path: &Path) {
    if let Some(s) = path.to_str() {
        env.entry(key.to_owned()).or_insert_with(|| s.to_owned());
    } else {
        warn!(key, path = %path.display(), "non-UTF-8 path not injected into sandbox env");
    }
}

/// Command child entry point: enter the daemon namespace, set up the sandbox, and fork init.
///
/// Runs in the forked child from [`run_attach`]. Sequence:
/// 1. Enter the daemon's user+mount namespace via `setns`
/// 2. Build the isolated root filesystem (`overlay::setup`)
/// 3. Create UTS namespace and set hostname (requires `CAP_SYS_ADMIN`)
/// 4. Create PID namespace (`unshare_pid`)
/// 5. Inject control socket env var if present
/// 6. Fork the init process (becomes PID 1 in the new PID namespace)
/// 7. Wait for init to exit and propagate its exit code
///
/// This function never returns — it calls `exit()` via [`run_or_exit`].
fn command_main(
    ns: Namespace,
    paths: &CommandPaths<'_>,
    command: &[OsString],
    sandbox: &SandboxConfig,
    identity: HostIdentity,
) {
    run_or_exit("command", || {
        ns.enter()?;
        debug!("entered daemon namespace");

        overlay::setup(
            paths.fuse_path,
            &sandbox.bind_mounts,
            &sandbox.state_root,
            &sandbox.mount_root,
        )?;

        // Set hostname before dropping root — NEWUTS requires CAP_SYS_ADMIN.
        namespace::unshare_uts(&sandbox.hostname)?;

        // PID namespace: the next fork creates PID 1.
        namespace::unshare_pid()?;

        let command = command.to_vec();
        let mut sandbox_env = sandbox.env.clone();

        // Inject env vars for nested nyne invocations: the control
        // socket (for `nyne exec`) and the session dir (for nested
        // `nyne mount`/`nyne list` to find each other's sessions).
        inject_path_env(&mut sandbox_env, control::NYNE_CONTROL_SOCKET_ENV, paths.control_socket);
        inject_path_env(&mut sandbox_env, control::NYNE_SESSION_DIR_ENV, paths.session_dir);
        let init_pid = fork_or_die(|| {
            init_main(&command, identity, paths, &sandbox_env);
        })
        .wrap_err("forking init (PID 1)")?;

        let exit_code = wait_for_exit(init_pid).wrap_err("waiting for init")?;
        exit(exit_code);
    });
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
    paths: &CommandPaths<'_>,
    extra_env: &HashMap<String, String>,
) {
    run_or_exit("init", || {
        // Remount /proc for the new PID namespace. Must happen before
        // user remap — mounting requires CAP_SYS_ADMIN in the user
        // namespace that owns the mount namespace.
        mnt::proc(Path::new("/proc"))?;
        debug!("/proc remounted for PID namespace");

        // Drop root identity.
        namespace::unshare_user_remap(identity.uid, identity.gid)?;

        // Set CWD: remap to mount_root if inside the FUSE-mounted project,
        // fall back to mount_root if the target doesn't exist in the sandbox.
        if env::set_current_dir(sandbox_cwd(paths.cwd, paths.fuse_path, paths.mount_root)).is_err() {
            env::set_current_dir(paths.mount_root).wrap_err("setting working directory to mount root fallback")?;
        }
        debug!(cwd = %env::current_dir().unwrap_or_default().display(), "working directory set");

        for (key, value) in extra_env {
            process::set_env(key, value);
        }

        exit(process::run_init(command)?);
    });
}
