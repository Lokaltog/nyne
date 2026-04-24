use std::path::Path;

use rstest::rstest;
use serde_json::{Value, json};

use super::*;

/// Returns a hardcoded project root path for test assertions.
fn root() -> &'static Path { Path::new("/mnt/project") }

/// Tests that injected hooks merge correctly into an empty JSON object.
#[rstest]
fn merge_into_empty_object() {
    let base = json!({});
    let injected = injected_hooks(root());
    let result = merge_settings(base, injected).unwrap();

    let hooks = result.get("hooks").expect("hooks key exists");
    let session_start = hooks.get("SessionStart").expect("SessionStart exists");
    let arr = session_start.as_array().expect("is array");
    assert_eq!(arr.len(), 2, "status hook + skills guidance hook");

    let entry = &arr[0];
    assert_eq!(entry["matcher"], "startup|resume|clear");
    assert_eq!(entry["hooks"][0]["type"], "command");
    let cmd = entry["hooks"][0]["command"].as_str().unwrap();
    assert!(cmd.contains("/mnt/project/@/STATUS.md"), "command contains mount path");
}

/// Tests that existing JSON keys are preserved during hook injection.
#[rstest]
fn preserves_existing_keys() {
    let base = json!({
        "permissions": { "allow": ["Read"] },
        "model": "claude-sonnet-4-20250514"
    });
    let injected = injected_hooks(root());
    let result = merge_settings(base, injected).unwrap();

    assert_eq!(result["permissions"]["allow"][0], "Read");
    assert_eq!(result["model"], "claude-sonnet-4-20250514");
    assert!(result.get("hooks").is_some());
}

/// Tests that injected hooks append to existing `SessionStart` entries.
#[rstest]
fn appends_to_existing_session_start_hooks() {
    let base = json!({
        "hooks": {
            "SessionStart": [{
                "matcher": "startup",
                "hooks": [{ "type": "command", "command": "echo hello" }]
            }]
        }
    });
    let injected = injected_hooks(root());
    let result = merge_settings(base, injected).unwrap();

    let arr = result["hooks"]["SessionStart"].as_array().unwrap();
    assert_eq!(arr.len(), 3, "original entry + status hook + skills guidance hook");

    // Original entry preserved at index 0.
    assert_eq!(arr[0]["matcher"], "startup");
    assert_eq!(arr[0]["hooks"][0]["command"], "echo hello");

    // Nyne entry appended at index 1.
    assert_eq!(arr[1]["matcher"], "startup|resume|clear");
}

/// Tests that hooks for non-injected events are preserved unchanged.
#[rstest]
fn preserves_existing_hooks_for_other_events() {
    let base = json!({
        "hooks": {
            "PreToolUse": [{
                "matcher": "",
                "hooks": [{ "type": "command", "command": "lint" }]
            }]
        }
    });
    let injected = injected_hooks(root());
    let result = merge_settings(base, injected).unwrap();

    // PreToolUse: original entry preserved + nyne's 2 narrow scripts
    // (file-access, grep-symbol).
    let pre = result["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(pre.len(), 3);
    assert_eq!(pre[0]["hooks"][0]["command"], "lint");

    // SessionStart added (status + skills = 2 entries).
    let session = result["hooks"]["SessionStart"].as_array().unwrap();
    assert_eq!(session.len(), 2);
}


/// Verifies `render_settings` produces valid JSON with injected hooks for every
/// empty-input shape (missing file or empty bytes).
#[rstest]
#[case::no_real_file(None)]
#[case::empty_bytes(Some(&[] as &[u8]))]
fn render_settings_empty_inputs_produce_hooks(#[case] existing: Option<&[u8]>) {
    let result = render_settings(existing, root()).unwrap();
    let parsed: Value = serde_json::from_slice(&result).unwrap();
    assert!(parsed["hooks"]["SessionStart"].is_array());
}


/// Tests that existing JSON content is preserved when rendering settings.
#[rstest]
fn render_settings_with_existing_json() {
    let existing = br#"{"model": "fast"}"#;
    let result = render_settings(Some(existing), root()).unwrap();
    let parsed: Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(parsed["model"], "fast");
    assert!(parsed["hooks"]["SessionStart"].is_array());
}

/// Tests that invalid JSON input produces an error.
#[rstest]
fn render_settings_with_invalid_json_returns_error() {
    let bad = b"not json";
    let result = render_settings(Some(bad), root());
    assert!(result.is_err());
}

/// Tests that hook commands use jq for JSON envelope construction.
#[rstest]
fn hook_command_uses_jq_envelope() {
    let hooks = injected_hooks(root());
    let entries = hooks["SessionStart"].as_array().unwrap();
    let cmd = entries[0]["hooks"][0]["command"].as_str().unwrap();
    assert!(cmd.contains("jq -Rs"), "command uses jq for JSON envelope");
    assert!(
        cmd.contains("hookSpecificOutput"),
        "command produces hookSpecificOutput"
    );
    assert!(cmd.contains("additionalContext"), "command includes additionalContext");
}

/// Tests that `PostToolUse` injection does not interfere with `SessionStart` hooks.
#[rstest]
fn post_tool_use_does_not_interfere_with_session_start() {
    let base = json!({
        "hooks": {
            "SessionStart": [{
                "matcher": "startup",
                "hooks": [{ "type": "command", "command": "echo hello" }]
            }]
        }
    });
    let injected = injected_hooks(root());
    let result = merge_settings(base, injected).unwrap();

    // Original SessionStart entry + nyne SessionStart entry.
    let session = result["hooks"]["SessionStart"].as_array().unwrap();
    assert_eq!(session.len(), 3);

    // PostToolUse: 5 narrow-concern scripts (bash-hints, cli-alts,
    // vfs-reread, ssot, diagnostics).
    let post = result["hooks"]["PostToolUse"].as_array().unwrap();
    assert_eq!(post.len(), 5);
}

/// Tests that all hook commands use `nyne exec` for dispatch.
///
/// After the decomposition, `PreToolUse` and `PostToolUse` each register
/// multiple narrow scripts with distinct matchers. This asserts the
/// full post-decomposition registry wires every script's command line
/// correctly.
#[rstest]
fn hook_commands_use_nyne_exec() {
    let hooks = injected_hooks(root());

    let collect = |event: &str| -> Vec<String> {
        hooks[event]
            .as_array()
            .unwrap_or_else(|| panic!("{event} exists"))
            .iter()
            .map(|e| e["hooks"][0]["command"].as_str().unwrap().to_owned())
            .collect()
    };

    // PreToolUse: 2 narrow scripts (file-access, grep-symbol).
    let pre = collect("PreToolUse");
    assert_eq!(pre, vec![
        "nyne exec provider.claude.pre-tool-use-file-access".to_owned(),
        "nyne exec provider.claude.pre-tool-use-grep-symbol".to_owned(),
    ]);

    // PostToolUse: 5 narrow scripts (bash-hints, cli-alts, vfs-reread, ssot, diagnostics).
    let post = collect("PostToolUse");
    assert_eq!(post, vec![
        "nyne exec provider.claude.post-tool-use-bash-hints".to_owned(),
        "nyne exec provider.claude.post-tool-use-cli-alts".to_owned(),
        "nyne exec provider.claude.post-tool-use-vfs-reread".to_owned(),
        "nyne exec provider.claude.post-tool-use-ssot".to_owned(),
        "nyne exec provider.claude.post-tool-use-diagnostics".to_owned(),
    ]);

    // Stop: single script.
    let stop = collect("Stop");
    assert_eq!(stop, vec!["nyne exec provider.claude.stop".to_owned()]);
}
