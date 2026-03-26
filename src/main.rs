//! Binary entry point for the nyne CLI.
//!
//! This crate is intentionally thin -- it parses CLI arguments via [`Cli`],
//! sets up logging, and dispatches to the library-side subcommand handlers.
//! Plugin crates (`nyne_git`, `nyne_source`, `nyne_lsp`) are linked via `use ... as _`
//! so their `linkme` distributed-slice entries are discovered at link time
//! without any explicit registration code.

use std::{io, process};

use clap::Parser;
use color_eyre::eyre::Result;
use nyne::cli::{Cli, Command, attach, config, ctl, exec, list, mount};
// Ensure plugin crates are linked — their `linkme` distributed slice
// entries are discovered at link time.
use tracing_subscriber::EnvFilter;
use {nyne_analysis as _, nyne_claude as _, nyne_git as _, nyne_lsp as _, nyne_source as _, nyne_todo as _};

/// Entry point for the nyne CLI.
///
/// Sets up `color_eyre` for rich error reports, configures `tracing` based on
/// the `-v` verbosity flag, then dispatches to the appropriate subcommand's
/// `run()` function. Subcommands that return an exit code (`Attach`, `Exec`)
/// are forwarded to [`std::process::exit`]; the rest return `Result<()>`
/// directly.
///
/// The `tracing` filter defaults to warnings-only, with `-v` enabling
/// progressively more detail. `fuser::reply` is silenced at most levels
/// because its output is extremely noisy and rarely useful for debugging
/// nyne itself.
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
