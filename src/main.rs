use std::{io, process};

use clap::Parser;
use color_eyre::eyre::Result;
use nyne::cli::{Cli, Command, attach, config, ctl, exec, list, mount};
// Ensure plugin crates are linked — their `linkme` distributed slice
// entries are discovered at link time.
use nyne_coding as _;
use nyne_git as _;
use tracing_subscriber::EnvFilter;

/// Entry point for the nyne CLI.
fn main() -> Result<()> {
    color_eyre::install()?;

    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new(match cli.verbose {
                0 => "warn,fuser::reply=off",
                1 => "nyne=info,fuser::reply=off",
                2 => "nyne=debug,nyne::fuse=info,fuser::reply=off",
                _ => "nyne=trace",
            })
        }))
        .with_writer(io::stderr)
        .init();

    match &cli.command {
        Command::Mount(args) => mount::run(args),
        Command::Attach(args) => process::exit(attach::run(args)?),
        Command::List(args) => list::run(args),
        Command::Exec(args) => process::exit(exec::run(args)?),
        Command::Ctl(args) => ctl::run(args),
        Command::Config(args) => config::run(args),
    }
}
