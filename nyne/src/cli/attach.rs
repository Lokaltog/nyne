//! `nyne attach` -- enter the namespace of a running mount and execute a command.
//!
//! This module handles the lifecycle of an attached process: resolving the
//! target session, registering with the daemon for process tracking, entering
//! the sandbox namespace, and cleaning up on exit via [`RegistrationGuard`].

use std::ffi::OsString;
use std::path::PathBuf;
use std::{env, process};

use clap::Args;
use color_eyre::eyre::Result;
use tracing::{info, warn};

use crate::config::NyneConfig;
use crate::types::ProcessVisibility;
use crate::{plugin, sandbox, session};

/// Arguments for the `attach` subcommand.
///
/// Attaching enters the mount namespace of a running nyne daemon and executes
/// a command (defaulting to `$SHELL`) so the user gets an interactive session
/// where the FUSE overlay is visible. The spawned process is registered with
/// the daemon for tracking in `nyne list` and unregistered on exit.
#[derive(Debug, Args)]
pub struct AttachArgs {
    #[command(flatten)]
    pub(crate) session: super::SessionArgs,

    /// Virtual filesystem visibility for the spawned process and its children.
    ///
    /// - `all`: force all nyne nodes (including companion dirs) into directory listings.
    /// - `default`: normal nyne behavior -- companion dirs hidden from listings.
    /// - `none`: full passthrough -- process sees only the real filesystem.
    #[arg(long, default_value = "default")]
    pub visibility: ProcessVisibility,

    /// Command to execute inside the namespace. Defaults to $SHELL.
    #[arg(last = true)]
    pub command: Vec<OsString>,
}

/// RAII guard that registers a process with the daemon on creation and
/// unregisters it on drop.
///
/// This ensures the daemon's process table stays accurate even if the attach
/// command panics or returns early via `?`. Without this guard, orphaned
/// entries would accumulate in `nyne list` output.
struct RegistrationGuard {
    socket: PathBuf,
    pid: i32,
}

impl RegistrationGuard {
    /// Register this process with the daemon and return a drop guard.
    ///
    /// Sends a `Register` control request so that `nyne list` can display
    /// the attached process. Returns `None` if the request fails -- this is
    /// intentionally non-fatal because the attach itself can still succeed;
    /// the user just won't see the process in listings.
    fn register(socket: PathBuf, pid: i32, command: String) -> Option<Self> {
        let req = sandbox::control::Request::Register { pid, command };
        if let Err(e) = sandbox::control::send_request(&socket, &req) {
            warn!(error = %e, "failed to register with daemon — nyne list may not show this process");
            return None;
        }
        Some(Self { socket, pid })
    }

    /// Set visibility override for this process.
    ///
    /// Tells the daemon how to present virtual filesystem entries to this
    /// PID and its children. Failures are logged as warnings rather than
    /// propagated, because a visibility failure should not prevent the
    /// attach session from working.
    fn set_visibility(&self, visibility: ProcessVisibility) {
        let req = sandbox::control::Request::SetVisibility {
            pid: Some(self.pid),
            name: None,
            visibility,
        };
        if let Err(e) = sandbox::control::send_request(&self.socket, &req) {
            warn!(error = %e, "failed to set visibility — using default");
        }
    }
}

impl Drop for RegistrationGuard {
    /// Unregister this process from the daemon's process table.
    ///
    /// Best-effort: if the daemon is already gone (e.g., killed), the
    /// unregister request will fail silently with a warning log.
    fn drop(&mut self) {
        let req = sandbox::control::Request::Unregister { pid: self.pid };
        if let Err(e) = sandbox::control::send_request(&self.socket, &req) {
            warn!(error = %e, "failed to unregister from daemon");
        }
    }
}

/// Run the attach subcommand: enter a running mount's namespace and execute a command.
///
/// Resolves the target session, registers the process with the daemon for
/// `nyne list` tracking, optionally overrides visibility, then delegates to
/// [`sandbox::run_attach`] which performs the actual namespace entry (joining
/// the daemon's mount/PID/UTS namespaces).
///
/// The spawned command defaults to `$SHELL` when no explicit command is given,
/// providing an interactive shell session inside the FUSE overlay.
///
/// # Errors
///
/// Returns an error if session resolution fails or if sandbox entry fails.
/// Registration and visibility failures are non-fatal (logged as warnings).
pub fn run(args: &AttachArgs) -> Result<i32> {
    let config = NyneConfig::load(&plugin::instantiate(), None)?;
    let session_info = args.session.resolve()?;
    let control_socket = session::control_socket(&session_info.id)
        .inspect_err(|e| warn!(error = %e, "control socket unavailable — process registration disabled"))
        .ok();

    let command = if args.command.is_empty() {
        vec![env::var_os("SHELL").unwrap_or_else(|| OsString::from("/bin/sh"))]
    } else {
        args.command.clone()
    };

    info!(
        id = %session_info.id,
        path = %session_info.mount_path.display(),
        command = ?command,
        "attaching to daemon"
    );

    // Register this process with the daemon so `nyne list` can show it.
    // The guard unregisters automatically on drop (panic, early return, or normal exit).
    let guard = control_socket.as_ref().and_then(|socket| {
        RegistrationGuard::register(
            socket.clone(),
            process::id().cast_signed(),
            command
                .first()
                .map(|c| c.to_string_lossy().into_owned())
                .unwrap_or_default(),
        )
    });

    // Set visibility if non-default — applies to this PID and its children.
    if args.visibility != ProcessVisibility::Default
        && let Some(g) = &guard
    {
        g.set_visibility(args.visibility);
    }

    sandbox::run_attach(sandbox::AttachConfig {
        daemon_pid: session_info.pid,
        mount_path: session_info.mount_path,
        control_socket,
        command,
        sandbox: config.sandbox,
    })
}
