//! `PreToolUse` hook — intercepts tool calls before execution.
//!
//! All rendering logic lives in `templates/pre-tool-use.md.j2` (master)
//! which dispatches to per-mode partials in `templates/pre-tool-use/`.
//! This module computes derived fields and picks the mode.

use std::fs;
use std::path::Path;
use std::time::SystemTime;

use nyne::dispatch::script::{Script, ScriptContext};
use nyne::prelude::*;
use nyne::templates::TemplateEngine;
use nyne_source::providers::names;
use nyne_source::services::SourceServices;
use nyne_source::syntax::find_fragment_at_line;
use nyne_source::syntax::fragment::Fragment;
use nyne_source::syntax::view::{SYMBOL_TABLE_PARTIAL_KEY, SYMBOL_TABLE_PARTIAL_SRC, fragment_list};

use crate::config::PreToolHookConfig;
use crate::provider::hook_schema::{
    EditToolInput, GrepToolInput, HookInput, HookOutput, ReadToolInput, WriteToolInput,
};
use crate::provider::settings::RAW_FILE_GRACE_SECS;

/// Master template key for pre-tool-use hook.
const TMPL_PRE: &str = "claude/pre-tool-use";
/// Partial template key for deny response.
const PARTIAL_DENY: &str = "hooks/pre-tool-use/deny";
/// Partial template key for VFS hint response.
const PARTIAL_HINT: &str = "hooks/pre-tool-use/hint";
/// Partial template key for grep symbol hint.
const PARTIAL_GREP: &str = "hooks/pre-tool-use/grep";

/// `PreToolUse` hook script implementation.
///
/// Intercepts Read, Edit, Write, Bash, and Grep tool calls before execution.
/// For file-access tools, computes whether the target has a VFS decomposition
/// and either emits a hint (suggesting the `@/` namespace) or denies the
/// raw access. For Grep, detects symbol-search patterns and suggests LSP
/// alternatives like `CALLERS.md` or `REFERENCES.md`.
pub(in crate::provider) struct PreToolUse {
    engine: Arc<TemplateEngine>,
    config: PreToolHookConfig,
}

/// Constructor for [`PreToolUse`].
impl PreToolUse {
    /// Create a new pre-tool-use hook with registered templates.
    pub fn new(config: &PreToolHookConfig) -> Self {
        let mut b = names::handle_builder();
        b.register_partial(SYMBOL_TABLE_PARTIAL_KEY, SYMBOL_TABLE_PARTIAL_SRC);
        b.register_partial(super::PARTIAL_VFS_HINTS, super::PARTIAL_VFS_HINTS_SRC);
        b.register_partial(PARTIAL_DENY, include_str!("../templates/pre-tool-use/deny.md.j2"));
        b.register_partial(PARTIAL_HINT, include_str!("../templates/pre-tool-use/hint.md.j2"));
        b.register_partial(PARTIAL_GREP, include_str!("../templates/pre-tool-use/grep.md.j2"));
        b.register(TMPL_PRE, include_str!("../templates/pre-tool-use.md.j2"));
        Self {
            engine: b.finish(),
            config: config.clone(),
        }
    }
}

/// [`Script`] implementation for [`PreToolUse`].
impl Script for PreToolUse {
    /// Parse hook input and dispatch to the appropriate handler.
    fn exec(&self, ctx: &ScriptContext<'_>, stdin: &[u8]) -> Result<Vec<u8>> {
        let Some(input) = HookInput::parse(stdin) else {
            return Ok(HookOutput::empty());
        };
        let tool_name = input.tool_name.as_deref().unwrap_or("");

        match tool_name {
            "Read" | "Edit" | "Write" => Ok(self.handle_file_access(ctx, &input, tool_name)),
            "Grep" => Ok(self.render_grep(&input)),
            _ => Ok(HookOutput::empty()),
        }
    }
}

/// Rendering and file access interception methods.
impl PreToolUse {
    /// Render a VFS symbol hint for grep patterns that look like symbol searches.
    fn render_grep(&self, input: &HookInput) -> Vec<u8> {
        let Some(pattern) = input.tool_input_as::<GrepToolInput>().and_then(|g| g.pattern) else {
            return HookOutput::empty();
        };
        let Some((kind, symbol)) = extract_symbol_from_grep(&pattern) else {
            return HookOutput::empty();
        };
        let mode = "grep";
        let rendered = self
            .engine
            .render(TMPL_PRE, &minijinja::context! { mode, kind, symbol });
        let trimmed = rendered.trim();
        if trimmed.is_empty() {
            HookOutput::empty()
        } else {
            HookOutput::context("PreToolUse", trimmed.to_owned()).to_bytes()
        }
    }

    /// File access interception — computes decomposition context, picks hint vs deny.
    fn handle_file_access(&self, ctx: &ScriptContext<'_>, input: &HookInput, tool_name: &str) -> Vec<u8> {
        // Deserialize typed inputs once for reuse across sections.
        let read_input = (tool_name == "Read")
            .then(|| input.tool_input_as::<ReadToolInput>())
            .flatten();
        let edit_input = (tool_name == "Edit")
            .then(|| input.tool_input_as::<EditToolInput>())
            .flatten();
        let write_input = (tool_name == "Write")
            .then(|| input.tool_input_as::<WriteToolInput>())
            .flatten();

        let file_path = match tool_name {
            "Read" => read_input.as_ref().and_then(|r| r.file_path.clone()),
            "Edit" => edit_input.as_ref().and_then(|e| e.file_path.clone()),
            "Write" => write_input.and_then(|w| w.file_path),
            _ => return HookOutput::empty(),
        };

        let Some(file_path) = file_path else {
            return HookOutput::empty();
        };

        // Only intercept paths under the mount root, skip @/ virtual paths.
        let activation = ctx.activation();
        let root_prefix = activation.root_prefix();
        let Some(rel) = file_path.strip_prefix(root_prefix) else {
            return HookOutput::empty();
        };
        if super::is_vfs_path(&file_path) {
            return HookOutput::empty();
        }

        // Only intercept files that have symbol decomposition.
        let Ok(vfs_path) = VfsPath::new(rel) else {
            return HookOutput::empty();
        };
        let Ok(decomposed) = SourceServices::get(activation).decomposition.get(&vfs_path) else {
            return HookOutput::empty();
        };
        if decomposed.decomposed.is_empty() {
            return HookOutput::empty();
        }

        // Within grace period → pass through silently.
        let overlay = activation.overlay_root();
        if is_within_grace(overlay, rel) {
            return HookOutput::empty();
        }
        stamp_atime(overlay, rel);

        // Resolve per-filetype policy.
        let policy = self.config.resolve(decomposed.decomposer.language_name());

        // Resolve which symbol the tool call targets.
        let sym = match tool_name {
            "Read" => {
                let offset = read_input.as_ref().and_then(|r| r.offset).unwrap_or(0);
                resolve_symbol_at_line(
                    &decomposed.decomposed,
                    usize::try_from(offset).unwrap_or(usize::MAX),
                    &decomposed.source,
                )
            }
            "Edit" => edit_input
                .and_then(|e| e.old_string)
                .and_then(|old| find_line_of_string(overlay, rel, &old))
                .and_then(|line| {
                    let line = usize::try_from(line).unwrap_or(usize::MAX);
                    resolve_symbol_at_line(&decomposed.decomposed, line.saturating_sub(1), &decomposed.source)
                }),
            _ => None,
        };

        // File metadata for templates.
        let total_lines = decomposed.source.lines().count();
        let total_bytes = decomposed.source.len();
        let ext = Path::new(rel).extension().and_then(|e| e.to_str()).unwrap_or("");

        // Pick mode: deny broad reads, hint everything else.
        let mode = if tool_name == "Read" {
            let narrow = read_input
                .and_then(|r| r.limit)
                .is_some_and(|l| l.cast_signed() <= policy.narrow_read_limit());
            let threshold = policy.deny_lines_threshold();
            let under_threshold = threshold < 0 || i64::try_from(total_lines).unwrap_or(i64::MAX) < threshold;
            if narrow || under_threshold { "hint" } else { "deny" }
        } else {
            "hint"
        };

        let config = minijinja::Value::from_serialize(&policy);
        let fragments = fragment_list(&decomposed.decomposed, &decomposed);
        let view = minijinja::context! { mode, tool_name, rel, sym, ext, fragments, config, total_lines, total_bytes };
        let rendered = self.engine.render(TMPL_PRE, &view);
        let trimmed = rendered.trim();

        if trimmed.is_empty() {
            return HookOutput::empty();
        }

        match mode {
            "deny" => HookOutput::deny(trimmed.to_owned()).to_bytes(),
            _ => HookOutput::context("PreToolUse", trimmed.to_owned()).to_bytes(),
        }
    }
}

// Grep: pattern classification

/// Detect if a grep pattern is searching for symbol usage and extract the symbol name.
///
/// Returns `(kind, symbol)` where kind is "callers", "references", or "imports".
fn extract_symbol_from_grep(pattern: &str) -> Option<(&'static str, String)> {
    // Qualified path: Foo::bar → suggest the method (last identifier)
    if pattern.contains("::") {
        let sym = pattern
            .split("::")
            .last()?
            .trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
        if !sym.is_empty() {
            return Some(("callers", sym.to_owned()));
        }
    }

    // Method call: \.method( or fn method
    if pattern.starts_with("\\.") || pattern.starts_with("fn ") || pattern.starts_with("fn\\s") {
        let sym = extract_first_identifier(pattern)?;
        return Some(("callers", sym));
    }

    // Bare function call: word\( or word(
    if pattern.contains("\\(") || (pattern.contains('(') && pattern.chars().next().is_some_and(char::is_alphabetic)) {
        let sym = extract_first_identifier(pattern)?;
        return Some(("callers", sym));
    }

    // PascalCase type name (all alphanumeric, starts with uppercase)
    if pattern.starts_with(|c: char| c.is_ascii_uppercase()) && pattern.chars().all(|c| c.is_alphanumeric() || c == '_')
    {
        return Some(("references", pattern.to_owned()));
    }

    // Import statement
    if pattern.starts_with("use ") || pattern.starts_with("import ") || pattern.starts_with("from ") {
        return Some(("imports", String::new()));
    }

    None
}

/// Extract the first word-like identifier from a pattern, skipping regex syntax.
fn extract_first_identifier(pattern: &str) -> Option<String> {
    let start = pattern.find(|c: char| c.is_alphanumeric() || c == '_')?;
    let rest = &pattern[start..];
    let ident: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    if ident == "fn" {
        return extract_first_identifier(&rest[ident.len()..]);
    }
    if ident.is_empty() { None } else { Some(ident) }
}

// Shared helpers

/// Check if the file's atime is within the grace period.
///
/// After denying a raw file read, the hook stamps the file's atime via
/// [`stamp_atime`]. Subsequent accesses within [`RAW_FILE_GRACE_SECS`]
/// are allowed through without a hint/deny, preventing an annoying
/// loop when the agent retries immediately after being redirected.
fn is_within_grace(overlay_root: &Path, rel: &str) -> bool {
    let real_path = overlay_root.join(rel);
    let Ok(meta) = fs::metadata(&real_path) else {
        return false;
    };
    let Ok(atime) = meta.accessed() else {
        return false;
    };
    let Ok(elapsed) = SystemTime::now().duration_since(atime) else {
        return false;
    };
    elapsed.as_secs() < RAW_FILE_GRACE_SECS
}

/// Stamp atime to suppress re-triggering within the grace period.
fn stamp_atime(overlay_root: &Path, rel: &str) {
    let _ = filetime::set_file_atime(overlay_root.join(rel), filetime::FileTime::now());
}

/// Find the 1-based line number of the first occurrence of `needle` in a file.
fn find_line_of_string(overlay_root: &Path, rel: &str, needle: &str) -> Option<u64> {
    let content = fs::read_to_string(overlay_root.join(rel)).ok()?;
    let first_line = needle.lines().next()?;
    content
        .lines()
        .position(|line| line.contains(first_line))
        .map(|i| (i + 1) as u64)
}

/// Resolve a 0-based line number to a VFS symbol `fs_name` path.
fn resolve_symbol_at_line(fragments: &[Fragment], line: usize, source: &str) -> Option<String> {
    Some(find_fragment_at_line(fragments, line, source)?.join(nyne::VFS_SEPARATOR))
}
#[cfg(test)]
mod tests;
