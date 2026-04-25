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
use nyne::path_utils::PathExt;
use nyne::templates::TemplateEngine;
use nyne::{Script, ScriptContext};

use crate::plugin::config::StopHookConfig;
use crate::provider::hook_schema::{HookInput, HookOutput};

/// Template key for the stop hook.
const TMPL_STOP: &str = "claude/stop";

/// Stop hook script implementation.
///
/// Scans the Claude Code transcript for Edit/Write tool uses in the
/// current turn. If code changes are detected, blocks the stop and
/// emits a review prompt listing modified files, reminding the agent
/// to verify SSOT/DRY compliance before concluding.
pub(in crate::provider) struct Stop {
    pub(in crate::provider) engine: Arc<TemplateEngine>,
    pub(in crate::provider) config: StopHookConfig,
}

pub(in crate::provider) fn build_engine() -> Arc<TemplateEngine> {
    let mut b = super::hook_builder();
    b.register(TMPL_STOP, include_str!("../templates/stop.md.j2"));
    b.finish()
}
/// [`Script`] implementation for [`Stop`].
impl Script for Stop {
    /// Scan transcript for changes and render a review prompt if needed.
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

        let root = ctx.activation().root();
        let changed_files = find_changed_files(Path::new(transcript_path), root, &self.config.ignore_extensions);

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
///
/// Performs a single pass over the transcript: when a human prompt line is
/// encountered, accumulated paths are cleared so only changes after the
/// final prompt survive.
fn find_changed_files(transcript: &Path, root: &Path, ignore_extensions: &[String]) -> Vec<String> {
    let Ok(file) = File::open(transcript) else {
        return Vec::new();
    };

    let mut files = BTreeSet::new();
    let mut saw_prompt = false;

    for line in BufReader::new(file).lines() {
        let Ok(line) = line else { break };

        // Detect human prompt — reset accumulated paths so only the last
        // turn's changes survive.
        if line.contains("\"type\":\"user\"")
            && line.contains("\"promptId\"")
            && !line.contains("\"tool_result\"")
            && !line.contains("\"isMeta\"")
        {
            files.clear();
            saw_prompt = true;
            continue;
        }

        if !saw_prompt || !line.contains("\"type\":\"assistant\"") {
            continue;
        }

        extract_changed_paths(&line, root, ignore_extensions, &mut files);
    }

    files.into_iter().collect()
}

/// Extract file paths from Edit/Write `tool_use` blocks in a single transcript
/// line, filtering out ignored extensions.
fn extract_changed_paths(line: &str, root: &Path, ignore_extensions: &[String], out: &mut BTreeSet<String>) {
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
        let rel = Path::new(fp).strip_root_str(root).unwrap_or(fp);
        if is_ignored_extension(rel, ignore_extensions) {
            continue;
        }
        out.insert(rel.to_owned());
    }
}

/// Check whether a file path has an extension in the ignore list.
fn is_ignored_extension(path: &str, ignore_extensions: &[String]) -> bool {
    let Some(ext) = Path::new(path).extension_str() else {
        return false;
    };
    ignore_extensions
        .iter()
        .any(|ignored| ignored.eq_ignore_ascii_case(ext))
}

/// Unit tests.
#[cfg(test)]
mod tests;
