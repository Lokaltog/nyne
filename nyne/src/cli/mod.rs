pub mod attach;
pub mod config;
pub mod exec;
pub mod list;
pub mod mount;
pub mod output;

use clap::{ArgAction, Parser, Subcommand};

use self::attach::AttachArgs;
use self::config::ConfigArgs;
use self::exec::ExecArgs;
use self::list::ListArgs;
use self::mount::MountArgs;

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
    /// Show the effective configuration with all defaults resolved.
    Config(ConfigArgs),
}
