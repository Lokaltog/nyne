//! Claude Code hook implementations as [`Script`] trait objects.
//!
//! One script per hook event type — each handles all tool matchers
//! internally rather than splitting into per-tool shell scripts.

/// Post-tool-use hook -- fires after tool execution completes.
mod post_tool_use;
/// Pre-tool-use hook -- intercepts tool calls before execution.
mod pre_tool_use;
/// Session start hook -- surfaces VFS guidance and project context.
mod session_start;
/// Statusline script -- renders ANSI status bar from JSON payload.
mod statusline;
/// Stop hook -- SSOT/DRY review after turns with code changes.
mod stop;

pub(in crate::providers::claude) use post_tool_use::PostToolUse;
pub(in crate::providers::claude) use pre_tool_use::PreToolUse;
pub(in crate::providers::claude) use session_start::SessionStart;
pub(in crate::providers::claude) use statusline::Statusline;
pub(in crate::providers::claude) use stop::Stop;

/// Shared partial template key for VFS hint macros.
/// Shared partial template key for VFS hint macros.
const PARTIAL_VFS_HINTS: &str = "hooks/vfs-hints";
/// Shared partial template source for VFS hint macros.
/// Shared partial template source for VFS hint macros.
const PARTIAL_VFS_HINTS_SRC: &str = include_str!("templates/vfs-hints.j2");

use crate::providers::names::{FILE_OVERVIEW, VFS_SEP, VFS_SYMBOLS_SEP};

/// Check whether a raw file path is a VFS virtual path.
/// Check whether a raw file path is a VFS virtual path.
fn is_vfs_path(path: &str) -> bool { path.contains(VFS_SEP) }

/// Extract the real source file path from a VFS path (everything before the first `@/`).
/// Extract the real source file path from a VFS path (before the first `@/`).
fn source_file_of(path: &str) -> &str { path.split(VFS_SEP).next().unwrap_or(path) }

/// Extract the symbol name from a VFS path like `file.rs@/symbols/Foo@/body.rs`.
/// Extract the symbol name from a VFS path like `file.rs@/symbols/Foo@/body.rs`.
fn symbol_from_vfs_path(path: &str) -> Option<&str> {
    let after_symbols = path.split(VFS_SYMBOLS_SEP).nth(1)?;
    let name = after_symbols.split(VFS_SEP).next()?;
    if name.is_empty() { None } else { Some(name) }
}

/// Check whether a path points to a symbols OVERVIEW.md.
/// Check whether a path points to a symbols OVERVIEW.md.
fn is_symbols_overview(path: &str) -> bool { path.contains(VFS_SYMBOLS_SEP) && path.ends_with(FILE_OVERVIEW) }
