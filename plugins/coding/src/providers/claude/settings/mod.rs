//! Claude Code user settings and configuration.

use std::path::Path;

use color_eyre::eyre::Result;
use nyne::json::deep_merge;
use serde_json::{Map, Value, json};

/// A hook entry within a Claude Code settings hook event array.
///
/// Represents one matcher + hooks pair, e.g.:
/// ```json
/// { "matcher": "startup|resume", "hooks": [{ "type": "command", "command": "..." }] }
/// ```
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
/// Hook entry in settings configuration.
pub(super) struct HookEntry {
    pub matcher: String,
    pub hooks: Vec<HookAction>,
}

/// A single hook action (the inner `hooks` array element).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(tag = "type")]
/// Action associated with a hook (launch, attach, message).
pub(super) enum HookAction {
    #[serde(rename = "command")]
    Command { command: String },
}

/// Grace period (seconds) after a refused raw file access during which
/// subsequent accesses are allowed through. Stored as atime via `touch -a`.
pub(in crate::providers::claude) const RAW_FILE_GRACE_SECS: u64 = 300;

/// Build the hooks that nyne injects into `settings.local.json`.
///
/// Returns a map of event name → list of hook entries. This is the **single
/// source of truth** for all nyne-managed hooks.
///
/// Hook commands use `nyne exec` to invoke native Rust scripts registered
/// by the `CodingPlugin` via [`Plugin::scripts()`](nyne::plugin::Plugin::scripts).
pub(super) fn injected_hooks(root: &Path) -> Map<String, Value> {
    let entry = |matcher: &str, name: &str| HookEntry {
        matcher: matcher.into(),
        hooks: vec![HookAction::Command {
            command: format!("nyne exec provider.claude.{name}"),
        }],
    };

    let mut hooks = Map::new();

    // SessionStart: surface mount status + skill guidance.
    let status_path = root.join("@/STATUS.md");
    hooks.insert(
        "SessionStart".into(),
        json!([
            HookEntry {
                matcher: "startup|resume|clear".into(),
                hooks: vec![HookAction::Command {
                    command: format!(
                        r#"cat {path} | jq -Rs '{{hookSpecificOutput: {{hookEventName: "SessionStart", additionalContext: .}}}}'"#,
                        path = status_path.display(),
                    ),
                }],
            },
            entry("startup|resume|clear", "session-start"),
        ]),
    );

    // PreToolUse: block broad raw reads, hint on targeted reads + edits/writes,
    // suggest LSP analysis for grep.
    hooks.insert(
        "PreToolUse".into(),
        json!([entry("Read|Edit|Write|Grep", "pre-tool-use")]),
    );

    // PostToolUse: VFS nudges, inline diagnostics, SSOT/DRY checks,
    // tool redirects (rg/fd over Grep/Glob).
    hooks.insert(
        "PostToolUse".into(),
        json!([entry("Bash|Edit|Write|Read|Grep|Glob", "post-tool-use")]),
    );

    // Stop: SSOT/DRY review after turns with code changes.
    hooks.insert("Stop".into(), json!([entry("", "stop")]));

    hooks
}

/// Merge nyne-managed hooks into a settings value.
///
/// Rules:
/// - If `base` has no `hooks` key, creates it with only our entries.
/// - If `base.hooks` exists, iterates our injected event arrays and **appends**
///   each entry to the corresponding existing array (or creates a new array).
/// - All other keys in `base` are preserved untouched.
pub(super) fn merge_settings(mut base: Value, injected: Map<String, Value>) -> Result<Value> {
    use color_eyre::eyre::ensure;

    let obj = base
        .as_object_mut()
        .ok_or_else(|| color_eyre::eyre::eyre!("settings root must be a JSON object"))?;

    let hooks_obj = obj
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| color_eyre::eyre::eyre!("hooks must be a JSON object"))?;

    for (event_name, entries) in injected {
        let Value::Array(new_entries) = entries else {
            continue;
        };

        let existing = hooks_obj.entry(&event_name).or_insert_with(|| Value::Array(Vec::new()));
        ensure!(existing.is_array(), "hook event '{event_name}' must be a JSON array");
        let arr = existing
            .as_array_mut()
            .ok_or_else(|| color_eyre::eyre::eyre!("unreachable"))?;
        arr.extend(new_entries);
    }

    Ok(base)
}

/// Nyne-managed default settings for Claude Code.
///
/// These are the single source of truth for settings values that nyne
/// controls. The real `.claude/settings.json` on disk can override any
/// of these — user values always win.
fn default_settings() -> Value {
    json!({
        "$schema": "https://json.schemastore.org/claude-code-settings.json",
        "outputStyle": "Principal SWE",
        "env": {
            "CLAUDE_CODE_DISABLE_AUTO_MEMORY": "1",
            "CLAUDE_CODE_DISABLE_BACKGROUND_TASKS": "1",
            "CLAUDE_CODE_DISABLE_CRON": "1",
            "CLAUDE_CODE_DISABLE_GIT_INSTRUCTIONS": "1",
            "CLAUDE_CODE_HIDE_ACCOUNT_INFO": "1",
            "CLAUDE_CODE_MAX_OUTPUT_TOKENS": "128000",
            "ENABLE_CLAUDEAI_MCP_SERVERS": "false",
            // "ENABLE_TOOL_SEARCH": "auto:1", // NOTE: temporarily disabled for testing
            "SLASH_COMMAND_TOOL_CHAR_BUDGET": "1",
            "USE_BUILTIN_RIPGREP": "0",
        },
        "includeGitInstructions": false,
        "attribution": { "commit": "", "pr": "" },
        "permissions": { "defaultMode": "bypassPermissions",
            "deny": [
                "CronCreate",
                "CronDelete",
                "CronList",
                "EnterPlanMode",
                "EnterWorktree",
                "ExitPlanMode",
                "LSP",
                "ListMcpResourcesTool",
                "NotebookEdit",
                "ReadMcpResourceTool",
                "TodoWrite",
                "ToolSearch",
                "WorktreeCreate",
                "WorktreeRemove",
            ]
        },
        "companyAnnouncements": [
            "You are running in a \x1b[1mnyne\x1b[0m managed environment, enjoy!",
        ],
        "skipDangerousModePermissionPrompt": true,
        "statusLine": { "type": "command", "command": "nyne exec provider.claude.statusline" },
        "spinnerVerbs": { "mode": "replace", "verbs": [ "Nyneing" ] },
        "spinnerTipsOverride": { "excludeDefault": true, "tips": [
            "nyne decomposes markdown files into sections — each header becomes a virtual file you can read and edit independently",
            "Use `mv` on symbol directories for LSP-powered rename across your entire codebase",
            "Preview any mutation as a .diff before applying — `cat file.rs@/symbols/Foo@/rename/bar.diff`",
            "nyne memories persist across sessions — agents write observations to @/memories/ and get them back automatically",
            "Standard coreutils compose with nyne: cat, grep, diff, sed, find, xargs — all work on virtual files",
            "Compare symbols across branches: `diff <(cat @/git/branches/main/src/lib.rs@/symbols/Foo.rs) <(cat @/git/branches/feature/src/lib.rs@/symbols/Foo.rs)`",
            "Apply LSP code actions with `cp file.rs@/symbols/Foo@/actions/*.diff file.rs@/patch/`",
            "Reorder markdown sections by renaming: `mv doc.md@/symbols/50-appendix.md doc.md@/symbols/20-appendix.md`",
            "Add an import by copying a symbol: `cp other.rs@/symbols/Config@/ file.rs@/symbols/imports/`",
            "Delete a symbol cleanly with `rmdir file.rs@/symbols/Foo@/`",
        ] },
        // "spinnerTipsEnabled": true, // NOTE: temporarily disabled for testing
    })
}

/// Produce the final `settings.json` content by layering:
/// 1. Nyne defaults (from [`default_settings`])
/// 2. Real `.claude/settings.json` from disk (user overrides, deep-merged)
/// 3. Nyne-injected hooks (always appended, never overridden)
pub(super) fn render_settings(real_json: Option<&[u8]>, root: &Path) -> Result<Vec<u8>> {
    let mut base = default_settings();

    if let Some(bytes) = real_json.filter(|b| !b.is_empty()) {
        let user: Value = serde_json::from_slice(bytes)?;
        deep_merge(&mut base, &user);
    }

    let merged = merge_settings(base, injected_hooks(root))?;
    let output = serde_json::to_string_pretty(&merged)?;
    Ok(output.into_bytes())
}

/// Unit tests.
#[cfg(test)]
mod tests;
