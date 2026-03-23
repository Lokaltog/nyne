pub mod attach;
pub mod config;
pub mod ctl;
pub mod exec;
pub mod list;
pub mod mount;
pub mod output;

use std::env;
use std::path::PathBuf;

use clap::{ArgAction, Parser, Subcommand};
use color_eyre::eyre::Result;

use self::attach::AttachArgs;
use self::config::ConfigArgs;
use self::ctl::CtlArgs;
use self::exec::ExecArgs;
use self::list::ListArgs;
use self::mount::MountArgs;
use crate::sandbox;
use crate::session::{self, SessionRegistry};

/// nyne — expose source code as a FUSE filesystem.
#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
pub struct Cli {
    /// Increase log verbosity (-v info, -vv debug, -vvv trace).
    #[arg(short, long, action = ArgAction::Count, global = true)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Mount one or more directories as FUSE filesystems.
    Mount(MountArgs),
    /// Attach to a running mount and execute a command in its namespace.
    Attach(AttachArgs),
    /// List active sessions and attached processes.
    List(ListArgs),
    /// Execute a registered script via a daemon's control socket.
    Exec(ExecArgs),
    /// Send a control request to a running daemon.
    Ctl(CtlArgs),
    /// Show the effective configuration with all defaults resolved.
    Config(ConfigArgs),
}

/// Discover the control socket path for a session.
///
/// Priority:
/// 1. Explicit `--id` flag → derive socket from session ID
/// 2. `NYNE_CONTROL_SOCKET` env var → use directly
/// 3. Single active session → use its socket
/// 4. Error
pub(crate) fn discover_socket(id: Option<&str>) -> Result<PathBuf> {
    if let Some(id) = id {
        return session::control_socket(id);
    }

    if let Ok(socket) = env::var(sandbox::control::NYNE_CONTROL_SOCKET_ENV) {
        return Ok(PathBuf::from(socket));
    }

    session::control_socket(&SessionRegistry::scan()?.resolve(None)?.id)
}
