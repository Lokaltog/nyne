//! `nyne ctl` -- generic JSON control interface to a running daemon.
//!
//! This is the low-level escape hatch for interacting with a daemon's control
//! socket. Unlike purpose-built subcommands (`list`, `exec`), `ctl` accepts
//! arbitrary [`sandbox::control::Request`] JSON and prints the raw
//! [`sandbox::control::Response`]. Useful for debugging and scripting.

use std::io::{self, Write};

use clap::Args;
use color_eyre::eyre::{Result, WrapErr};
use tracing::info;

use crate::sandbox;

/// Arguments for the `ctl` subcommand.
///
/// Accepts a JSON control request either as a positional argument or via
/// stdin (for larger payloads or piped usage). The session is resolved
/// through the shared [`SessionArgs`](super::SessionArgs) flattened fields.
#[derive(Debug, Args)]
pub struct CtlArgs {
    #[command(flatten)]
    session: super::SessionArgs,

    /// Control request JSON. Reads from stdin if omitted.
    pub request: Option<String>,
}

/// Send a control request to a running daemon and print the JSON response.
///
/// Deserializes the request JSON (from the argument or stdin), sends it over
/// the Unix domain socket to the daemon's control server, and pretty-prints
/// the response to stdout. This is intentionally thin -- all request/response
/// types are defined in [`sandbox::control`], making `ctl` a pure passthrough.
///
/// # Errors
///
/// Returns an error if the JSON is malformed, the socket is unreachable, or
/// the response cannot be serialized.
pub fn run(args: &CtlArgs) -> Result<()> {
    let socket_path = args.session.socket_path()?;

    let req: sandbox::control::Request = match &args.request {
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
