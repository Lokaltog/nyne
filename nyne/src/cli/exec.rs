//! `nyne exec` -- pipe-oriented script execution.
//!
//! Executes a named script registered by a provider in a running daemon.
//! Designed for binary stdin/stdout piping: reads all of stdin, sends it
//! to the daemon along with the script address, and writes the raw response
//! bytes to stdout. Exit code is always 0 on success.

use std::io::{self, Read};

use clap::Args;
use color_eyre::eyre::{Result, WrapErr};
use tracing::info;

use crate::sandbox;

/// Arguments for the `exec` subcommand.
///
/// The `address` identifies a script registered by a provider at activation
/// time (e.g., `provider.claude.post-tool-use`). The session is resolved
/// through the shared [`SessionArgs`](super::SessionArgs) flattened fields.
#[derive(Debug, Args)]
pub struct ExecArgs {
    #[command(flatten)]
    session: super::SessionArgs,

    /// Script address to execute (e.g., `provider.claude.post-tool-use`).
    pub address: String,
}

/// Execute a registered script via a daemon's control socket.
///
/// Reads all of stdin into memory, sends it as the script's input payload
/// over the control socket, and writes the script's stdout bytes to this
/// process's stdout. This binary-in/binary-out design allows `nyne exec`
/// to be used in shell pipelines (e.g., `echo '{}' | nyne exec addr`).
///
/// # Errors
///
/// Returns an error if stdin cannot be read, the control socket is
/// unreachable, or the script execution fails on the daemon side.
pub fn run(args: &ExecArgs) -> Result<i32> {
    let socket_path = args.session.socket_path()?;

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
