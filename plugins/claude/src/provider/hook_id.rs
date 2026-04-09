//! Stable identifiers for nyne-injected Claude Code hook scripts.
//!
//! [`HookId`] is the single source of truth used by:
//!
//! - [`HOOK_REGISTRY`](super::hooks::HOOK_REGISTRY) — declarative metadata
//!   + `Script` constructor for each hook.
//! - [`HooksToggle`](crate::plugin::config::HooksToggle) — per-script
//!   enable/disable map keyed by [`HookId`].
//! - `injected_hooks` in `provider::settings` — serializes kebab-case
//!   script names into `settings.json` hook entries.
//!
//! The kebab-case string form is the stable wire identifier written to
//! user-visible surfaces (settings.json, config.toml, script addresses).

use serde::{Deserialize, Serialize};

/// Stable identifier for a nyne-injected Claude Code hook script.
///
/// Serializes to kebab-case (`pre-tool-use-file-access`, etc.), which is
/// both the Claude Code script address suffix and the TOML toggle key.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    strum::Display,
    strum::EnumString,
    strum::EnumIter,
    strum::AsRefStr,
)]
#[strum(serialize_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum HookId {
    /// Pre-tool-use file-access guard (Read/Edit/Write hint vs deny).
    PreToolUseFileAccess,
    /// Pre-tool-use Grep symbol-search heuristic.
    PreToolUseGrepSymbol,
    /// Post-tool-use Bash per-binary VFS alternatives.
    PostToolUseBashHints,
    /// Post-tool-use Grep/Glob CLI alternatives.
    PostToolUseCliAlts,
    /// Post-tool-use VFS re-read reminder after writes.
    PostToolUseVfsReread,
    /// Post-tool-use SSOT/DRY reminder on significant edits.
    PostToolUseSsot,
    /// Post-tool-use LSP diagnostics + static analysis.
    PostToolUseDiagnostics,
    /// Post-tool-use-failure VFS rename ENOENT recovery.
    PostToolUseFailure,
    /// Session-start mount status + project context.
    SessionStart,
    /// Stop-hook SSOT/DRY review after turns with code changes.
    Stop,
    /// Statusline renderer (wired via `statusLine.command`, not a hook event).
    Statusline,
}
