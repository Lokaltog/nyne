//! Claude Code hook implementations as [`Script`] trait objects.
//!
//! One script per hook event type — each handles all tool matchers
//! internally rather than splitting into per-tool shell scripts.

/// Post-tool-use hook -- fires after tool execution completes.
pub(in crate::provider) mod post_tool_use;
/// Post-tool-use failure hook -- fires when an Edit tool call fails.
pub(in crate::provider) mod post_tool_use_failure;

/// Pre-tool-use hook -- intercepts tool calls before execution.
pub(in crate::provider) mod pre_tool_use;

/// Session start hook -- surfaces VFS guidance and project context.
pub(in crate::provider) mod session_start;

/// Statusline script -- renders ANSI status bar from JSON payload.
mod statusline;

mod util;

/// Stop hook -- SSOT/DRY review after turns with code changes.
pub(in crate::provider) mod stop;

use std::path::Path;
use std::sync::Arc;

use nyne::path_utils::PathExt;
use nyne::router::{Chain, Op};
use nyne::templates::{HandleBuilder, TemplateEngine};
use nyne::{Script, ScriptEntry, provider_script_address};
use nyne_companion::{Companion, CompanionRequest};
pub(in crate::provider) use post_tool_use::bash_hints::BashHints;
pub(in crate::provider) use post_tool_use::cli_alts::CliAlts;
pub(in crate::provider) use post_tool_use::diagnostics::Diagnostics;
pub(in crate::provider) use post_tool_use::ssot::Ssot;
pub(in crate::provider) use post_tool_use::vfs_reread::VfsReread;
pub(in crate::provider) use post_tool_use_failure::PostToolUseFailure;
pub(in crate::provider) use pre_tool_use::file_access::FileAccess;
pub(in crate::provider) use pre_tool_use::grep_symbol::GrepSymbol;
pub(in crate::provider) use session_start::SessionStart;
pub(in crate::provider) use statusline::Statusline;
pub(in crate::provider) use stop::Stop;

use crate::plugin::config::Config;
use crate::provider::hook_id::HookId;
use crate::provider::hook_schema::HookOutput;
use crate::provider::templates_shared;

/// Create a [`HandleBuilder`] with shared template partials pre-registered.
///
/// Thin alias over [`templates_shared::new_builder`] — every hook
/// script's `build_engine` calls this before registering its own
/// template, so every hook template has access to the same macros and
/// shared partials as the provider-side engine.
pub(super) fn hook_builder() -> HandleBuilder { templates_shared::new_builder() }

/// Render a hook template and wrap non-empty output as a context message.
///
/// Shared epilogue for hook `exec` methods: render → trim → empty-check →
/// `HookOutput::context(event_name, trimmed).to_bytes()`.
pub(super) fn render_context(
    engine: &TemplateEngine,
    tmpl: &str,
    view: &impl serde::Serialize,
    event_name: &'static str,
) -> Vec<u8> {
    let trimmed = engine.render(tmpl, view).trim().to_owned();
    if trimmed.is_empty() {
        HookOutput::empty()
    } else {
        HookOutput::context(event_name, trimmed).to_bytes()
    }
}

/// Resolve a file path through the middleware chain to determine VFS status.
///
/// Returns `Some(Companion)` if the path targets a VFS companion namespace,
/// `None` for regular filesystem paths. The caller can read
/// `companion.source_file` to get the underlying source file path.
pub(super) fn resolve_companion(chain: &Chain, root: &Path, abs_path: &Path) -> Option<Companion> {
    chain
        .evaluate(abs_path.strip_root(root)?.to_path_buf(), Op::Readdir)
        .ok()?
        .companion()
        .cloned()
}
/// Declarative registration entry for a Claude Code hook script.
///
/// Single source of truth for hook metadata — consumed by both
/// [`script_entries`] (to dispatch `nyne exec`) and
/// `provider::settings::injected_hooks` (to render the `settings.json`
/// hooks array). Adding a new hook is one entry in [`HOOK_REGISTRY`].
///
/// `event`/`matcher` control Claude Code's event dispatch pre-filtering.
/// `build` constructs the [`Script`] trait object on plugin activation.
/// Both function pointers (no captures) — zero runtime cost beyond the
/// indirect call.
pub(in crate::provider) struct HookDef {
    /// Stable identifier for this hook — SSOT used by the registry,
    /// toggle map, and `nyne exec` script address.
    pub id: HookId,
    /// Claude Code event name (e.g. `"PreToolUse"`).
    pub event: &'static str,
    /// Pipe-separated tool/event matcher (e.g. `"Read|Edit|Write"`).
    /// Empty string matches all events.
    pub matcher: &'static str,
    /// [`Script`] constructor invoked once per plugin activation.
    pub build: fn(&Config) -> Arc<dyn Script>,
}

/// Declarative hook registry — the single source of truth for hook
/// metadata and dispatch.
///
/// Every nyne-injected Claude Code hook script has one entry here,
/// keyed by [`HookId`]. Consumed by:
///
/// - [`script_entries`] — builds the per-script [`Script`] trait objects
///   registered on plugin activation.
/// - `provider::settings::injected_hooks` — renders the `settings.json`
///   `hooks` map (one event → many entries, accumulated per event).
///
/// Adding a new hook is one entry here (plus the corresponding
/// [`HookId`] variant). No separate binding or sync test required.
///
/// Note: [`HookId::Statusline`] is omitted — it's wired via
/// `statusLine.command` in `default_settings`, not through the hook
/// array.
pub(in crate::provider) const HOOK_REGISTRY: &[HookDef] = &[
    HookDef {
        id: HookId::PreToolUseFileAccess,
        event: "PreToolUse",
        matcher: "Read|Edit|Write",
        build: |c| {
            Arc::new(self::FileAccess {
                engine: self::pre_tool_use::file_access::build_engine(),
                config: c.hook_config.pre_tool.clone(),
            })
        },
    },
    HookDef {
        id: HookId::PreToolUseGrepSymbol,
        event: "PreToolUse",
        matcher: "Grep",
        build: |_| {
            Arc::new(self::GrepSymbol {
                engine: self::pre_tool_use::grep_symbol::build_engine(),
            })
        },
    },
    HookDef {
        id: HookId::PostToolUseBashHints,
        event: "PostToolUse",
        matcher: "Bash",
        build: |_| {
            Arc::new(self::BashHints {
                engine: self::post_tool_use::bash_hints::build_engine(),
            })
        },
    },
    HookDef {
        id: HookId::PostToolUseCliAlts,
        event: "PostToolUse",
        matcher: "Grep|Glob",
        build: |_| {
            Arc::new(self::CliAlts {
                engine: self::post_tool_use::cli_alts::build_engine(),
            })
        },
    },
    HookDef {
        id: HookId::PostToolUseVfsReread,
        event: "PostToolUse",
        matcher: "Edit|Write",
        build: |_| {
            Arc::new(self::VfsReread {
                engine: self::post_tool_use::vfs_reread::build_engine(),
            })
        },
    },
    HookDef {
        id: HookId::PostToolUseSsot,
        event: "PostToolUse",
        matcher: "Edit|Write",
        build: |_| {
            Arc::new(self::Ssot {
                engine: self::post_tool_use::ssot::build_engine(),
            })
        },
    },
    HookDef {
        id: HookId::PostToolUseDiagnostics,
        event: "PostToolUse",
        matcher: "Edit|Write",
        build: |_| {
            Arc::new(self::Diagnostics {
                engine: self::post_tool_use::diagnostics::build_engine(),
            })
        },
    },
    HookDef {
        id: HookId::PostToolUseFailure,
        event: "PostToolUseFailure",
        matcher: "Edit",
        build: |_| {
            Arc::new(self::PostToolUseFailure {
                engine: self::post_tool_use_failure::build_engine(),
            })
        },
    },
    HookDef {
        id: HookId::SessionStart,
        event: "SessionStart",
        matcher: "startup|resume|clear",
        build: |_| {
            Arc::new(self::SessionStart {
                engine: self::session_start::build_engine(),
            })
        },
    },
    HookDef {
        id: HookId::Stop,
        event: "Stop",
        matcher: "",
        build: |c| {
            Arc::new(self::Stop {
                engine: self::stop::build_engine(),
                config: c.hook_config.stop.clone(),
            })
        },
    },
];

/// Script entries registered by the source plugin on behalf of `ClaudeProvider`.
///
/// Respects both the master `claude.enabled` toggle (returns empty if
/// disabled) and individual per-script toggles in
/// [`HooksToggle`](crate::plugin::config::HooksToggle). Derives every
/// entry from [`HOOK_REGISTRY`] — one per enabled [`HookDef`] plus the
/// statusline script (wired outside the hook event array).
pub fn script_entries(config: &Config) -> Vec<ScriptEntry> {
    if !config.enabled {
        return Vec::new();
    }

    let addr = |id: HookId| provider_script_address("claude", id.as_ref());
    let mut entries: Vec<ScriptEntry> = HOOK_REGISTRY
        .iter()
        .filter(|def| config.hooks.is_enabled(def.id))
        .map(|def| (addr(def.id), (def.build)(config)))
        .collect();

    if config.hooks.is_enabled(HookId::Statusline) {
        entries.push((addr(HookId::Statusline), Arc::new(self::Statusline)));
    }

    entries
}
