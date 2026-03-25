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
const PARTIAL_VFS_HINTS: &str = "hooks/vfs-hints";
/// Shared partial template source for VFS hint macros.
const PARTIAL_VFS_HINTS_SRC: &str = include_str!("templates/vfs-hints.j2");

pub(super) use crate::providers::names::{is_symbols_overview, is_vfs_path, source_file_of, symbol_from_vfs_path};
