use std::ffi::OsString;
use std::path::PathBuf;
use std::{env, process};

use clap::Args;
use color_eyre::eyre::Result;
use tracing::{info, warn};

use crate::config::NyneConfig;
use crate::sandbox;
use crate::session::{self, SessionRegistry};
use crate::types::ProcessVisibility;

/// Arguments for the `attach` subcommand.
#[derive(Debug, Args)]
pub struct AttachArgs {
    /// Session ID to attach to (optional if only one mount is active).
    pub id: Option<String>,

    /// Virtual filesystem visibility for the spawned process and its children.
    ///
    /// - `all`: force all nyne nodes (including companion dirs) into directory listings.
    /// - `default`: normal nyne behavior — companion dirs hidden from listings.
    /// - `none`: full passthrough — process sees only the real filesystem.
    #[arg(long, default_value = "default")]
    pub visibility: ProcessVisibility,

    /// Command to execute inside the namespace. Defaults to $SHELL.
    #[arg(last = true)]
    pub command: Vec<OsString>,
}

/// RAII guard that registers a process with the daemon on creation and
/// unregisters it on drop. Ensures cleanup even if the attach command
/// panics or returns early via `?`.
struct RegistrationGuard {
    socket: PathBuf,
    pid: i32,
}

impl RegistrationGuard {
    /// Send a `Register` request to the daemon and return a guard that will
    /// `Unregister` on drop. Returns `None` if the registration request fails
    /// (logged as a warning).
    fn register(socket: PathBuf, pid: i32, command: String) -> Option<Self> {
        let req = sandbox::control::ControlRequest::Register { pid, command };
        if let Err(e) = sandbox::control::send_request(&socket, &req) {
            warn!(error = %e, "failed to register with daemon — nyne list may not show this process");
            return None;
        }
        Some(Self { socket, pid })
    }

    /// Set visibility override for this process. Failures are logged as warnings.
    fn set_visibility(&self, visibility: ProcessVisibility) {
        let req = sandbox::control::ControlRequest::SetVisibility {
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
    fn drop(&mut self) {
        let req = sandbox::control::ControlRequest::Unregister { pid: self.pid };
        if let Err(e) = sandbox::control::send_request(&self.socket, &req) {
            warn!(error = %e, "failed to unregister from daemon");
        }
    }
}

/// Run the attach subcommand: attach to a running mount and execute a command in its namespace.
pub fn run(args: &AttachArgs) -> Result<i32> {
    let registry = SessionRegistry::scan()?;
    let session_info = registry.resolve(args.id.as_deref())?;
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
        mount_path: session_info.mount_path.clone(),
        control_socket,
        command,
        sandbox: NyneConfig::load()?.sandbox,
    })
}
