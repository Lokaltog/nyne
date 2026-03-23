use std::ffi::OsString;
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

/// Run the attach subcommand: attach to a running mount and execute a command in its namespace.
pub fn run(args: &AttachArgs) -> Result<i32> {
    let registry = SessionRegistry::scan()?;
    let session_info = registry.resolve(args.id.as_deref())?;

    let control_socket = session::control_socket(&session_info.id).ok();

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
    let pid = process::id().cast_signed();
    let command_name = command
        .first()
        .map(|c| c.to_string_lossy().into_owned())
        .unwrap_or_default();
    if let Some(socket) = &control_socket {
        let req = sandbox::control::ControlRequest::Register {
            pid,
            command: command_name,
        };
        if let Err(e) = sandbox::control::send_request(socket, &req) {
            warn!(error = %e, "failed to register with daemon — nyne list may not show this process");
        }

        // Set visibility if non-default — applies to this PID and its children.
        if args.visibility != ProcessVisibility::Default {
            let req = sandbox::control::ControlRequest::SetVisibility {
                pid: Some(pid),
                name: None,
                visibility: args.visibility,
            };
            if let Err(e) = sandbox::control::send_request(socket, &req) {
                warn!(error = %e, "failed to set visibility — using default");
            }
        }
    }

    let nyne_config = NyneConfig::load()?;

    let config = sandbox::AttachConfig {
        daemon_pid: session_info.pid,
        mount_path: session_info.mount_path.clone(),
        control_socket: control_socket.clone(),
        command,
        sandbox: nyne_config.sandbox,
    };

    let result = sandbox::run_attach(config);

    // Best-effort unregister on exit.
    if let Some(socket) = &control_socket {
        let req = sandbox::control::ControlRequest::Unregister { pid };
        let _ = sandbox::control::send_request(socket, &req);
    }

    result
}
