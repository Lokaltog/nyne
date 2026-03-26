//! `nyne list` -- show active sessions and their attached processes.
//!
//! Without arguments, lists all active nyne mount sessions. When a session ID
//! is provided, lists the processes currently attached to that session (as
//! tracked by the daemon's control server via [`RegistrationGuard`](super::attach::RegistrationGuard)).

use std::time::SystemTime;

use clap::Args;
use color_eyre::eyre::{Result, eyre};

use super::output::{self, Term, style};
use crate::sandbox;
use crate::session::{self, SessionRegistry};

/// Arguments for the `list` subcommand.
///
/// When no session is specified, all active sessions are listed. When `--id`
/// is provided, the attached processes for that specific session are shown
/// instead.
#[derive(Debug, Args)]
pub struct ListArgs {
    #[command(flatten)]
    session: super::SessionArgs,
}

/// Dispatch the list subcommand based on whether a session ID was provided.
///
/// Scans the session registry first, then either lists all sessions or drills
/// into a specific session's attached processes. Output is written to the
/// terminal via [`output::term()`].
pub fn run(args: &ListArgs) -> Result<()> {
    let term = output::term();
    let registry = SessionRegistry::scan()?;

    if let Some(id) = args.session.id() {
        return list_processes(&term, id);
    }

    list_sessions(&term, &registry)
}

/// Print a table of all active nyne sessions (ID, PID, mount path).
///
/// Shows a dimmed "no active sessions" message when the registry is empty,
/// rather than printing an empty table. This is the default view when
/// `nyne list` is invoked without arguments.
fn list_sessions(term: &Term, registry: &SessionRegistry) -> Result<()> {
    let sessions = registry.sessions();

    if sessions.is_empty() {
        term.write_line(&style("No active nyne sessions.").dim().to_string())?;
        return Ok(());
    }

    term.write_line(&style(format!("{:<16} {:<8} PATH", "ID", "PID")).bold().to_string())?;
    for info in sessions {
        term.write_line(&format!(
            "{:<16} {:<8} {}",
            style(&info.id).cyan(),
            info.pid,
            style(info.mount_path.display()).green(),
        ))?;
    }
    Ok(())
}

/// Print a table of processes attached to a specific session.
///
/// Sends a `ListProcesses` control request to the daemon and formats the
/// response as a table with PID, command name, elapsed duration, and start
/// timestamp. The duration is computed relative to the current wall clock,
/// so it reflects how long the process has been attached.
///
/// # Errors
///
/// Returns an error if the control socket is unreachable or the daemon
/// responds with an error or unexpected response type.
fn list_processes(term: &Term, id: &str) -> Result<()> {
    let socket_path = session::control_socket(id)?;

    let req = sandbox::control::Request::ListProcesses;
    let resp = sandbox::control::send_request(&socket_path, &req)?;

    let sandbox::control::Response::Processes { list } = resp else {
        return match resp {
            sandbox::control::Response::Error { message } => Err(eyre!("daemon error: {message}")),
            other => Err(eyre!("unexpected response from daemon: {other:?}")),
        };
    };

    if list.is_empty() {
        term.write_line(
            &style(format!("No attached processes for session {id:?}."))
                .dim()
                .to_string(),
        )?;
        return Ok(());
    }

    term.write_line(
        &style(format!("{:<8} {:<24} {:<12} START", "PID", "COMMAND", "DURATION"))
            .bold()
            .to_string(),
    )?;
    for proc in &list {
        let elapsed = SystemTime::now().duration_since(proc.start_time).unwrap_or_default();

        term.write_line(&format!(
            "{:<8} {:<24} {:<12} {}",
            proc.pid,
            style(&proc.command).cyan(),
            humantime::format_duration(elapsed),
            style(humantime::format_rfc3339_seconds(proc.start_time)).dim(),
        ))?;
    }

    Ok(())
}
