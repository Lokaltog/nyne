use std::io::{self, Write};

use clap::Args;
use color_eyre::eyre::{Result, WrapErr};
use tracing::info;

use crate::sandbox;

/// Arguments for the `ctl` subcommand.
#[derive(Debug, Args)]
pub struct CtlArgs {
    /// Session ID (optional if only one mount is active).
    #[arg(long)]
    pub id: Option<String>,

    /// Control request JSON. Reads from stdin if omitted.
    pub request: Option<String>,
}

/// Send a control request to a running daemon and print the JSON response.
pub fn run(args: &CtlArgs) -> Result<()> {
    let socket_path = super::discover_socket(args.id.as_deref())?;

    let req: sandbox::control::ControlRequest = match &args.request {
        Some(json) => serde_json::from_str(json).wrap_err("parsing control request")?,
        None => serde_json::from_reader(io::stdin()).wrap_err("parsing control request from stdin")?,
    };

    info!(socket = %socket_path.display(), "sending control request");

    let resp = sandbox::control::send_request(&socket_path, &req)?;

    let mut stdout = io::stdout();
    serde_json::to_writer_pretty(&mut stdout, &resp).wrap_err("writing response")?;
    stdout.write_all(b"\n")?;

    Ok(())
}
