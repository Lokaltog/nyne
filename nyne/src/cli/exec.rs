use std::io::{self, Read};

use clap::Args;
use color_eyre::eyre::{Result, WrapErr};
use tracing::info;

use crate::sandbox;

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
    let socket_path = super::discover_socket(args.id.as_deref())?;

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
