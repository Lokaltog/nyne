use std::env;
use std::io::{self, Read};
use std::path::PathBuf;

use clap::Args;
use color_eyre::eyre::{Result, WrapErr};
use tracing::info;

use crate::sandbox;
use crate::session::{self, SessionRegistry};

/// Arguments for the `exec` subcommand.
#[derive(Debug, Args)]
pub struct ExecArgs {
    /// Session ID (optional if only one mount is active).
    #[arg(long)]
    pub id: Option<String>,

    /// Script address to execute (e.g., `provider.claude.post-tool-use`).
    pub address: String,
}

/// Run the exec subcommand: execute a registered script via a daemon's control socket.
pub fn run(args: &ExecArgs) -> Result<i32> {
    let socket_path = discover_socket(args.id.as_deref())?;

    info!(
        socket = %socket_path.display(),
        address = %args.address,
        "executing script"
    );

    let mut stdin = Vec::new();
    io::stdin().read_to_end(&mut stdin).wrap_err("reading stdin")?;

    let stdout = sandbox::control::exec_script(&socket_path, &args.address, &stdin)?;

    io::Write::write_all(&mut io::stdout(), &stdout).wrap_err("writing stdout")?;

    Ok(0)
}

/// Discover the control socket path.
///
/// Priority:
/// 1. Explicit `--id` flag → derive socket from session ID
/// 2. `NYNE_CONTROL_SOCKET` env var → use directly
/// 3. Single active session → use its socket
/// 4. Error
fn discover_socket(id: Option<&str>) -> Result<PathBuf> {
    if let Some(id) = id {
        return session::control_socket(id);
    }

    if let Ok(socket) = env::var(sandbox::control::NYNE_CONTROL_SOCKET_ENV) {
        return Ok(PathBuf::from(socket));
    }

    let registry = SessionRegistry::scan()?;
    let info = registry.resolve(None)?;
    session::control_socket(&info.id)
}
