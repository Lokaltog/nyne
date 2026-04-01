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
/// Stop hook -- SSOT/DRY review after turns with code changes.
pub(in crate::provider) mod stop;

use nyne::router::{Chain, Op};
use nyne::templates::{HandleBuilder, TemplateEngine};
use nyne::{Script, ScriptEntry, provider_script_address};
pub(in crate::provider) use post_tool_use::PostToolUse;
pub(in crate::provider) use post_tool_use_failure::PostToolUseFailure;
pub(in crate::provider) use pre_tool_use::PreToolUse;
pub(in crate::provider) use session_start::SessionStart;
pub(in crate::provider) use statusline::Statusline;
pub(in crate::provider) use stop::Stop;

use crate::plugin::config::Config;
use crate::provider::hook_schema::HookOutput;
use crate::provider::settings;

/// Shared partial template key for VFS hint macros.
///
/// Registered once and included by multiple hook templates to render
/// consistent VFS usage guidance across pre-tool-use and post-tool-use hooks.
const PARTIAL_VFS_HINTS: &str = "hooks/vfs-hints";
/// Shared partial template source for VFS hint macros.
///
/// Loaded at compile time from `templates/vfs-hints.j2`. The template
/// provides Jinja macros for rendering VFS path suggestions and symbol
/// navigation hints.
const PARTIAL_VFS_HINTS_SRC: &str = include_str!("templates/vfs-hints.j2");
/// Create a [`HandleBuilder`] with common hook partials pre-registered.
///
/// Wraps [`HandleBuilder::new()`] and registers the shared VFS hints
/// partial that multiple hooks include. Hook constructors should call this
/// instead of `HandleBuilder::new()` directly.
pub(super) fn hook_builder() -> HandleBuilder {
    let mut b = HandleBuilder::new();
    // TODO: replace hardcoded VFS names with VfsPathRegistry (see TODO_DERIVE_PATHS.md)
    let engine = b.engine_mut();
    engine.add_global("FILE_OVERVIEW", "OVERVIEW.md");
    engine.add_global("FILE_CALLERS", "CALLERS.md");
    engine.add_global("FILE_DEPS", "DEPS.md");
    engine.add_global("FILE_REFERENCES", "REFERENCES.md");
    engine.add_global("FILE_IMPLEMENTATION", "IMPLEMENTATION.md");
    b.register_partial(PARTIAL_VFS_HINTS, PARTIAL_VFS_HINTS_SRC);
    b
}

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
    let rendered = engine.render(tmpl, view);
    let trimmed = rendered.trim();
    if trimmed.is_empty() {
        HookOutput::empty()
    } else {
        HookOutput::context(event_name, trimmed.to_owned()).to_bytes()
    }
}
use std::path::PathBuf;

use nyne_companion::{Companion, CompanionRequest};

/// Check whether a path points to a symbols OVERVIEW.md (`@/symbols/` … `OVERVIEW.md`).
pub(super) fn is_symbols_overview(path: &str) -> bool { path.contains("@/symbols/") && path.ends_with("OVERVIEW.md") }

/// Resolve a file path through the middleware chain to determine VFS status.
///
/// Returns `Some(Companion)` if the path targets a VFS companion namespace,
/// `None` for regular filesystem paths. The caller can read
/// `companion.source_file` to get the underlying source file path.
pub(super) fn resolve_companion(chain: &Chain, root: &str, abs_path: &str) -> Option<Companion> {
    chain
        .evaluate(PathBuf::from(abs_path.strip_prefix(root)?), Op::Readdir)
        .ok()?
        .companion()
        .cloned()
}
/// Script entries registered by the source plugin on behalf of `ClaudeProvider`.
///
/// Respects both the master `claude.enabled` toggle (returns empty if disabled)
/// and individual `claude.hooks.*` toggles. Derives script names from
/// [`HOOK_REGISTRY`](settings::HOOK_REGISTRY).
pub fn script_entries(config: &Config) -> Vec<ScriptEntry> {
    use std::sync::Arc;

    if !config.enabled {
        return Vec::new();
    }

    let t = &config.hooks;
    let addr = |name| provider_script_address("claude", name);
    let mut entries: Vec<ScriptEntry> = Vec::new();

    for def in settings::HOOK_REGISTRY {
        if !match def.script_name {
            "pre-tool-use" => t.pre_tool_use,
            "post-tool-use" => t.post_tool_use,
            "post-tool-use-failure" => t.post_tool_use_failure,
            "session-start" => t.session_start,
            "stop" => t.stop,
            _ => continue,
        } {
            continue;
        }
        let script: Arc<dyn Script> = match def.script_name {
            "pre-tool-use" => Arc::new(self::PreToolUse {
                engine: self::pre_tool_use::build_engine(),
                config: config.hook_config.pre_tool.clone(),
            }),
            "post-tool-use" => Arc::new(self::PostToolUse {
                engine: self::post_tool_use::build_engine(),
            }),
            "post-tool-use-failure" => Arc::new(self::PostToolUseFailure {
                engine: self::post_tool_use_failure::build_engine(),
            }),
            "session-start" => Arc::new(self::SessionStart {
                engine: self::session_start::build_engine(),
            }),
            "stop" => Arc::new(self::Stop {
                engine: self::stop::build_engine(),
                config: config.hook_config.stop.clone(),
            }),
            _ => unreachable!(),
        };
        entries.push((addr(def.script_name), script));
    }

    // Statusline is wired via `statusLine.command` in default_settings,
    // not through the hooks array — registered separately.
    if t.statusline {
        entries.push((addr("statusline"), Arc::new(self::Statusline)));
    }

    entries
}
