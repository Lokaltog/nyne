//! `Stop` hook — SSOT/DRY review after turns with code changes.
//!
//! Scans the Claude Code transcript for Edit/Write tool uses in the current
//! turn and blocks the stop with a review prompt listing modified files.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::Arc;

use color_eyre::eyre::Result;
use nyne::dispatch::script::{Script, ScriptContext};
use nyne::templates::TemplateEngine;

use crate::config::StopHookConfig;
use crate::providers::claude::hook_schema::{HookInput, HookOutput};
use crate::providers::names;

const TMPL_STOP: &str = "claude/stop";

/// Stop hook script implementation.
pub(in crate::providers::claude) struct Stop {
    engine: Arc<TemplateEngine>,
    config: StopHookConfig,
}

impl Stop {
    pub fn new(config: &StopHookConfig) -> Self {
        let mut b = names::handle_builder();
        b.register(TMPL_STOP, include_str!("../templates/stop.md.j2"));
        Self {
            engine: b.finish(),
            config: config.clone(),
        }
    }
}

impl Script for Stop {
    fn exec(&self, ctx: &ScriptContext<'_>, stdin: &[u8]) -> Result<Vec<u8>> {
        let Some(input) = HookInput::parse(stdin) else {
            return Ok(HookOutput::empty());
        };

        // Guard against infinite loops — stop hooks can re-trigger themselves.
        if input.stop_hook_active == Some(true) {
            return Ok(HookOutput::empty());
        }

        let transcript_path = match input.transcript_path.as_deref() {
            Some(p) if !p.is_empty() => p,
            _ => return Ok(HookOutput::empty()),
        };

        let root_prefix = ctx.activation().root_prefix();
        let changed_files =
            find_changed_files(Path::new(transcript_path), &root_prefix, &self.config.ignore_extensions);

        if changed_files.len() < self.config.min_files {
            return Ok(HookOutput::empty());
        }

        let view = minijinja::context! { changed_files };
        let reason = self.engine.render(TMPL_STOP, &view);

        Ok(HookOutput::block(reason.trim().to_owned()).to_bytes())
    }
}

/// Scan the transcript for Edit/Write tool uses after the last human prompt.
///
/// Returns a sorted, deduplicated list of relative file paths, excluding any
/// whose extension appears in `ignore_extensions` (case-insensitive).
fn find_changed_files(transcript: &Path, root_prefix: &str, ignore_extensions: &[String]) -> Vec<String> {
    let Some(prompt_offset) = find_last_prompt_offset(transcript) else {
        return Vec::new();
    };

    let Ok(file) = File::open(transcript) else {
        return Vec::new();
    };

    let mut files = BTreeSet::new();
    let mut current_offset = 0u64;

    for line in BufReader::new(file).lines() {
        let Ok(line) = line else { break };
        let len = line.len() as u64 + 1;
        current_offset += len;
        if current_offset <= prompt_offset {
            continue;
        }

        // Look for assistant messages containing Edit/Write tool uses.
        if !line.contains("\"type\":\"assistant\"") {
            continue;
        }

        extract_changed_paths(&line, root_prefix, ignore_extensions, &mut files);
    }

    files.into_iter().collect()
}

/// Find the byte offset of the last human prompt in the transcript.
///
/// Human prompts have `"type":"user"` + `"promptId"` but NOT `"tool_result"`
/// or `"isMeta"`.
fn find_last_prompt_offset(transcript: &Path) -> Option<u64> {
    let file = File::open(transcript).ok()?;
    let mut last_offset = None;
    let mut offset = 0u64;

    for line in BufReader::new(&file).lines() {
        let Ok(line) = line else { break };
        let len = line.len() as u64 + 1; // +1 for newline
        if line.contains("\"type\":\"user\"")
            && line.contains("\"promptId\"")
            && !line.contains("\"tool_result\"")
            && !line.contains("\"isMeta\"")
        {
            last_offset = Some(offset);
        }
        offset += len;
    }

    last_offset
}

/// Extract file paths from Edit/Write `tool_use` blocks in a single transcript
/// line, filtering out ignored extensions.
fn extract_changed_paths(line: &str, root_prefix: &str, ignore_extensions: &[String], out: &mut BTreeSet<String>) {
    let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) else {
        return;
    };

    let Some(content) = msg
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())
    else {
        return;
    };

    for block in content {
        let is_edit_or_write = block.get("type").and_then(|t| t.as_str()) == Some("tool_use")
            && matches!(block.get("name").and_then(|n| n.as_str()), Some("Edit" | "Write"));
        if !is_edit_or_write {
            continue;
        }
        let Some(fp) = block
            .get("input")
            .and_then(|i| i.get("file_path"))
            .and_then(|p| p.as_str())
        else {
            continue;
        };
        let rel = fp.strip_prefix(root_prefix).unwrap_or(fp);
        if is_ignored_extension(rel, ignore_extensions) {
            continue;
        }
        out.insert(rel.to_owned());
    }
}

/// Check whether a file path has an extension in the ignore list.
fn is_ignored_extension(path: &str, ignore_extensions: &[String]) -> bool {
    let Some(ext) = Path::new(path).extension().and_then(|e| e.to_str()) else {
        return false;
    };
    ignore_extensions
        .iter()
        .any(|ignored| ignored.eq_ignore_ascii_case(ext))
}

#[cfg(test)]
mod tests;
