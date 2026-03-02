use std::{io, process};

use clap::Parser;
use color_eyre::eyre::Result;
use nyne::cli::{Cli, Command, attach, config, exec, list, mount};
// Ensure plugin crates are linked — their `linkme` distributed slice
// entries are discovered at link time.
use nyne_coding as _;
use nyne_git as _;
use tracing_subscriber::EnvFilter;

/// Entry point for the nyne CLI.
fn main() -> Result<()> {
    color_eyre::install()?;

    let cli = Cli::parse();

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(match cli.verbose {
            0 => "warn",
            1 => "nyne=info",
            2 => "nyne=debug,nyne::fuse=info",
            _ => "nyne=trace",
        })
    });

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(io::stderr)
        .init();

    match &cli.command {
        Command::Mount(args) => mount::run(args),
        Command::Attach(args) => {
            let exit_code = attach::run(args)?;
            process::exit(exit_code);
        }
        Command::List(args) => list::run(args),
        Command::Exec(args) => {
            let exit_code = exec::run(args)?;
            process::exit(exit_code);
        }
        Command::Config(args) => config::run(args),
    }
}
