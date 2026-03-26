//! CLI command implementations and shared argument types.
//!
//! Each subcommand lives in its own module (`attach`, `config`, `ctl`, `exec`,
//! `list`, `mount`) with a public `run()` entry point called from `main()`.
//! This module re-exports the top-level [`Cli`] parser and [`Command`] enum,
//! plus shared helpers for session resolution that multiple subcommands need.
//!
//! Terminal output is centralised in the [`output`] module -- all CLI modules
//! import `term()` and `style()` from there rather than using `println!` or
//! constructing `console::Term` directly.

/// CLI handler for `nyne attach` -- enter namespace of a running mount.
pub mod attach;
/// CLI handler for `nyne config` -- dump resolved configuration.
pub mod config;
/// CLI handler for `nyne ctl` -- generic JSON control interface to a running daemon.
pub mod ctl;
/// CLI handler for `nyne exec` -- pipe-oriented script execution.
pub mod exec;
/// CLI handler for `nyne list` -- show sessions and attached processes.
pub mod list;
/// CLI handler for `nyne mount` -- start FUSE daemon(s) for directory(ies).
pub mod mount;
/// Terminal output utilities -- single source of truth for CLI terminal access.
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

/// Top-level CLI argument parser for the nyne binary.
///
/// Parsed by `clap` in `main()` to extract the global verbosity flag and
/// the selected subcommand. Each [`Command`] variant delegates to its
/// module's `run()` function, keeping dispatch logic in `main.rs` thin.
///
/// The `verbose` counter controls `tracing` filter levels so that `-v`,
/// `-vv`, and `-vvv` progressively surface info, debug, and trace logs.
/// Without `-v`, only warnings are shown (plus `fuser::reply` is silenced
/// to avoid noise from the FUSE layer).
#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
pub struct Cli {
    /// Increase log verbosity (-v info, -vv debug, -vvv trace).
    #[arg(short, long, action = ArgAction::Count, global = true)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Command,
}

/// Top-level CLI subcommands dispatched by the binary entry point.
///
/// Each variant wraps the argument struct for its subcommand module. The
/// binary's `main()` matches on this enum and forwards to the corresponding
/// `run()` function. Variants that represent long-running processes (`Mount`)
/// block until interrupted, while interactive ones (`Attach`, `Exec`) return
/// an exit code that `main()` propagates via [`std::process::exit`].
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

/// Common arguments for commands that target a running session.
///
/// Embedded via `#[command(flatten)]` into subcommands that need to address a
/// specific daemon (e.g., `ctl`, `exec`). The optional `id` field allows the
/// user to specify which session to target; when omitted, [`discover_socket`]
/// auto-resolves if exactly one session is active.
#[derive(Debug, clap::Args)]
pub(crate) struct SessionArgs {
    /// Session ID (optional if only one mount is active).
    #[arg(long)]
    id: Option<String>,
}

impl SessionArgs {
    /// The raw session ID passed by the user, if any.
    pub(crate) fn id(&self) -> Option<&str> { self.id.as_deref() }

    /// Resolve the full [`session::SessionInfo`] for the targeted session.
    ///
    /// Delegates to [`resolve_session`], which scans the session registry and
    /// applies the standard resolution: explicit `--id` flag or single-session
    /// auto-detection.
    pub(crate) fn resolve(&self) -> Result<session::SessionInfo> { resolve_session(self.id.as_deref()) }

    /// Resolve the control socket path for the targeted session.
    ///
    /// Delegates to [`discover_socket`], which applies the priority chain:
    /// explicit `--id` flag, `NYNE_CONTROL_SOCKET` env var, or single-session
    /// auto-detection.
    pub(crate) fn socket_path(&self) -> Result<PathBuf> { discover_socket(self.id.as_deref()) }
}

/// Resolve a session by optional ID.
///
/// Scans the filesystem-based session registry for active nyne daemons and
/// returns the matching [`session::SessionInfo`]. This is the shared lookup
/// used by `attach` and `list` to find the target daemon.
///
/// # Errors
///
/// Returns an error if:
/// - No sessions are active and no `id` was provided.
/// - Multiple sessions are active and no `id` was provided (ambiguous).
/// - The requested `id` does not match any active session.
pub(super) fn resolve_session(id: Option<&str>) -> Result<session::SessionInfo> {
    Ok(SessionRegistry::scan()?.resolve(id)?.clone())
}

/// Discover the control socket path for a session.
///
/// Uses a priority chain so that the most explicit source wins:
///
/// 1. **Explicit `--id` flag** -- derive the socket path from the session ID.
/// 2. **`NYNE_CONTROL_SOCKET` env var** -- use the path directly. This is set
///    automatically inside sandbox namespaces so that attached processes can
///    reach their daemon without knowing the session ID.
/// 3. **Single active session** -- auto-resolve if exactly one daemon is running.
/// 4. **Error** -- none of the above matched; the user must specify `--id`.
fn discover_socket(id: Option<&str>) -> Result<PathBuf> {
    if let Some(id) = id {
        return session::control_socket(id);
    }

    if let Ok(socket) = env::var(sandbox::control::NYNE_CONTROL_SOCKET_ENV) {
        return Ok(PathBuf::from(socket));
    }

    session::control_socket(&resolve_session(None)?.id)
}
