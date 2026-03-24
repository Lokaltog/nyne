use std::time::SystemTime;

use clap::Args;
use color_eyre::eyre::Result;

use super::output::{self, Term, style};
use crate::sandbox;
use crate::session::{self, SessionRegistry};

/// Arguments for the `list` subcommand.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Session ID — if provided, list attached processes for that session only.
    pub id: Option<String>,
}

/// Run the list subcommand: list active sessions and attached processes.
pub fn run(args: &ListArgs) -> Result<()> {
    let term = output::term();
    let registry = SessionRegistry::scan()?;

    if let Some(id) = args.id.as_deref() {
        return list_processes(&term, id);
    }

    list_sessions(&term, &registry)
}

/// List all active sessions.
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

/// List all processes attached to a session.
fn list_processes(term: &Term, id: &str) -> Result<()> {
    let socket_path = session::control_socket(id)?;

    let req = sandbox::control::ControlRequest::ListProcesses;
    let resp = sandbox::control::send_request(&socket_path, &req)?;

    match resp {
        sandbox::control::ControlResponse::Processes { list } => {
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
                let duration = SystemTime::now().duration_since(proc.start_time).unwrap_or_default();
                let duration_str = humantime::format_duration(duration).to_string();
                let start_str = humantime::format_rfc3339_seconds(proc.start_time).to_string();

                term.write_line(&format!(
                    "{:<8} {:<24} {:<12} {}",
                    proc.pid,
                    style(&proc.command).cyan(),
                    duration_str,
                    style(start_str).dim(),
                ))?;
            }
        }
        sandbox::control::ControlResponse::Error { message } => {
            return Err(color_eyre::eyre::eyre!("daemon error: {message}"));
        }
        other => {
            return Err(color_eyre::eyre::eyre!("unexpected response from daemon: {other:?}"));
        }
    }

    Ok(())
}
