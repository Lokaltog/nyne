//! Claude Code user settings and configuration.
//!
//! Manages the `settings.json` file that Claude Code reads on startup.
//! Layers three sources: nyne-managed defaults (environment variables,
//! denied tools, hook registrations), user `settings.local.json`, and
//! dynamically injected hook scripts. The [`HOOK_REGISTRY`] is the single
//! source of truth for hook event names, matchers, and script paths.

use std::collections::BTreeMap;
use std::path::Path;

use color_eyre::eyre::Result;
use nyne::deep_merge::deep_merge;
use serde_json::{Map, Value, json};

use crate::provider::hooks::HOOK_REGISTRY;

/// A hook entry within a Claude Code settings hook event array.
///
/// Represents one matcher + hooks pair, e.g.:
/// ```json
/// { "matcher": "startup|resume", "hooks": [{ "type": "command", "command": "..." }] }
/// ```
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub(super) struct HookEntry {
    pub matcher: String,
    pub hooks: Vec<HookAction>,
}

/// A single hook action (the inner `hooks` array element).
///
/// Currently only `Command` is supported — the `type` field is used as
/// the serde tag discriminator matching Claude Code's JSON schema.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(tag = "type")]
pub(super) enum HookAction {
    #[serde(rename = "command")]
    Command { command: String },
}
/// Grace period (seconds) after a refused raw file access during which
/// subsequent accesses are allowed through. Stored as atime via `touch -a`.
pub(in crate::provider) const RAW_FILE_GRACE_SECS: u64 = 300;

/// Build the hooks that nyne injects into `settings.local.json`.
///
/// Returns a map of event name → list of hook entries. Derived from
/// [`HOOK_REGISTRY`](crate::provider::hooks::HOOK_REGISTRY) — adding a new
/// hook only requires a registry entry. Multiple registry entries sharing
/// the same `event` are accumulated under the same key (e.g., all six
/// `PostToolUse` scripts end up in a single array under `"PostToolUse"`).
///
/// Hook commands use `nyne exec` to invoke native Rust scripts registered
/// by the `SourcePlugin` via [`Plugin::scripts()`](nyne::plugin::Plugin::scripts).
pub(super) fn injected_hooks(root: &Path) -> Map<String, Value> {
    // Accumulate entries per event so multiple scripts sharing the same
    // event (e.g., the six PostToolUse scripts) coexist in one array.
    let mut per_event: BTreeMap<String, Vec<HookEntry>> = BTreeMap::new();
    let mut session_start_prepended = false;

    for def in HOOK_REGISTRY {
        let bucket = per_event.entry(def.event.into()).or_default();

        // SessionStart: prepend a STATUS.md cat command once, before any
        // SessionStart script entry. Multiple SessionStart scripts would
        // share the same preamble.
        if def.event == "SessionStart" && !session_start_prepended {
            bucket.push(HookEntry {
                matcher: def.matcher.into(),
                hooks: vec![HookAction::Command {
                    command: format!(
                        r#"cat {path} | jq -Rs '{{hookSpecificOutput: {{hookEventName: "SessionStart", additionalContext: .}}}}'"#,
                        path = root.join("@/STATUS.md").display(),
                    ),
                }],
            });
            session_start_prepended = true;
        }

        bucket.push(HookEntry {
            matcher: def.matcher.into(),
            hooks: vec![HookAction::Command {
                command: format!("nyne exec provider.claude.{}", def.id.as_ref()),
            }],
        });
    }

    per_event
        .into_iter()
        .map(|(event, entries)| (event, json!(entries)))
        .collect()
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

/// Environment variables injected into Claude Code's settings.
///
/// Each entry is `(key, value)`. Changing a value here is all that's needed —
/// the JSON structure in [`default_settings`] references this slice.
const ENV_VARS: &[(&str, &str)] = &[
    ("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1"),
    ("CLAUDE_CODE_DISABLE_BACKGROUND_TASKS", "1"),
    ("CLAUDE_CODE_DISABLE_CRON", "1"),
    ("CLAUDE_CODE_DISABLE_GIT_INSTRUCTIONS", "1"),
    ("CLAUDE_CODE_HIDE_ACCOUNT_INFO", "1"),
    ("CLAUDE_CODE_MAX_OUTPUT_TOKENS", "128000"),
    ("ENABLE_CLAUDEAI_MCP_SERVERS", "false"),
    ("SLASH_COMMAND_TOOL_CHAR_BUDGET", "1"),
];

/// Tool names denied in Claude Code's permission settings.
///
/// Sorted alphabetically. Add new entries here — the deny list in
/// [`default_settings`] is built from this slice.
const DENIED_TOOLS: &[&str] = &[
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
    "WorktreeCreate",
    "WorktreeRemove",
];

/// Nyne-managed default settings for Claude Code.
///
/// These are the single source of truth for settings values that nyne
/// controls. The real `.claude/settings.json` on disk can override any
/// of these — user values always win.
///
/// TODO: Migrate to plugin defaults (see LSP configuration)
fn default_settings() -> Value {
    let env: Map<String, Value> = ENV_VARS
        .iter()
        .map(|(k, v)| ((*k).to_owned(), Value::String((*v).to_owned())))
        .collect();

    let deny: Vec<Value> = DENIED_TOOLS.iter().map(|t| Value::String((*t).to_owned())).collect();

    json!({
        "$schema": "https://json.schemastore.org/claude-code-settings.json",
        "outputStyle": "Principal SWE",
        "env": env,
        "includeGitInstructions": false,
        "attribution": { "commit": "", "pr": "" },
        "permissions": { "defaultMode": "bypassPermissions", "deny": deny },
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
