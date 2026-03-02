use std::path::Path;

use serde_json::{Value, json};

use super::*;

fn root() -> &'static Path { Path::new("/mnt/project") }

#[test]
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

#[test]
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

#[test]
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

#[test]
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

    // PreToolUse: original entry preserved + nyne's single consolidated entry.
    let pre = result["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(pre.len(), 2);
    assert_eq!(pre[0]["hooks"][0]["command"], "lint");

    // SessionStart added (status + skills = 2 entries).
    let session = result["hooks"]["SessionStart"].as_array().unwrap();
    assert_eq!(session.len(), 2);
}

#[test]
fn render_settings_with_no_real_file() {
    let result = render_settings(None, root()).unwrap();
    let parsed: Value = serde_json::from_slice(&result).unwrap();
    assert!(parsed["hooks"]["SessionStart"].is_array());
}

#[test]
fn render_settings_with_empty_bytes() {
    let result = render_settings(Some(b""), root()).unwrap();
    let parsed: Value = serde_json::from_slice(&result).unwrap();
    assert!(parsed["hooks"]["SessionStart"].is_array());
}

#[test]
fn render_settings_with_existing_json() {
    let existing = br#"{"model": "fast"}"#;
    let result = render_settings(Some(existing), root()).unwrap();
    let parsed: Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(parsed["model"], "fast");
    assert!(parsed["hooks"]["SessionStart"].is_array());
}

#[test]
fn render_settings_with_invalid_json_returns_error() {
    let bad = b"not json";
    let result = render_settings(Some(bad), root());
    assert!(result.is_err());
}

#[test]
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

#[test]
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

    // PostToolUse: single consolidated entry.
    let post = result["hooks"]["PostToolUse"].as_array().unwrap();
    assert_eq!(post.len(), 1);
}

#[test]
fn hook_commands_use_nyne_exec() {
    let hooks = injected_hooks(root());

    // PostToolUse: single script handling all tools.
    let entries = hooks["PostToolUse"].as_array().expect("PostToolUse exists");
    assert_eq!(entries.len(), 1);
    let cmd = entries[0]["hooks"][0]["command"].as_str().unwrap();
    assert_eq!(cmd, "nyne exec provider.claude.post-tool-use");

    // PreToolUse: single script handling all tools.
    let pre_entries = hooks["PreToolUse"].as_array().expect("PreToolUse exists");
    assert_eq!(pre_entries.len(), 1);
    let cmd = pre_entries[0]["hooks"][0]["command"].as_str().unwrap();
    assert_eq!(cmd, "nyne exec provider.claude.pre-tool-use");

    // Stop: single script.
    let stop_entries = hooks["Stop"].as_array().expect("Stop exists");
    assert_eq!(stop_entries.len(), 1);
    let cmd = stop_entries[0]["hooks"][0]["command"].as_str().unwrap();
    assert_eq!(cmd, "nyne exec provider.claude.stop");
}
