//! `PostToolUse` hook — fires after tool execution completes.
//!
//! All rendering logic lives in `templates/post-tool-use.md.j2`.
//! This module computes derived fields that Jinja can't (command parsing,
//! VFS path extraction, line counting) and passes them alongside the raw
//! `tool_input` to the template.

use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use color_eyre::eyre::Result;
use nyne::dispatch::script::{Script, ScriptContext};
use nyne::templates::TemplateEngine;
use nyne::types::vfs_path::VfsPath;

use crate::lsp::diagnostic_view::{DiagnosticRow, diagnostics_to_rows};
use crate::providers::claude::hook_schema::{
    BashToolInput, EditToolInput, HookInput, HookOutput, ReadToolInput, WriteToolInput,
};
use crate::providers::names::{self, FILE_OVERVIEW};
use crate::services::CodingServices;
use crate::syntax::analysis::{AnalysisContext, AnalysisEngine, HintView};
use crate::syntax::decomposed::DecomposedSource;

/// Minimum combined old+new line count to trigger SSOT reminder on Edit.
const SSOT_LINE_THRESHOLD: usize = 10;

/// Template key for the post-tool-use hook.
const TMPL_POST: &str = "claude/post-tool-use";

/// `PostToolUse` hook script implementation.
pub(in crate::providers::claude) struct PostToolUse {
    engine: Arc<TemplateEngine>,
}

/// Methods for [`PostToolUse`].
impl PostToolUse {
    /// Create a new post-tool-use hook with registered templates.
    pub fn new() -> Self {
        let mut b = names::handle_builder();
        b.register_partial(super::PARTIAL_VFS_HINTS, super::PARTIAL_VFS_HINTS_SRC);
        b.register(TMPL_POST, include_str!("../templates/post-tool-use.md.j2"));
        Self { engine: b.finish() }
    }
}

/// [`Script`] implementation for [`PostToolUse`].
impl Script for PostToolUse {
    /// Process post-tool-use hook input and render context hints.
    fn exec(&self, ctx: &ScriptContext<'_>, stdin: &[u8]) -> Result<Vec<u8>> {
        let Some(input) = HookInput::parse(stdin) else {
            return Ok(HookOutput::empty());
        };
        let tool_name = input.tool_name.as_deref().unwrap_or("");
        let root = ctx.activation().root_prefix();

        let services = CodingServices::get(ctx.activation());
        let analysis = run_analysis_for_tool(&services.analysis, ctx, &input, tool_name, root);

        // Narrow hints/diagnostics to the changed region for Edit calls.
        let changed = analysis
            .decomposed
            .as_deref()
            .and_then(|d| changed_line_range(&input, tool_name, d));
        let hints = filter_hints(analysis.hints, changed.as_ref());
        let diagnostics = filter_diagnostics(
            fetch_diagnostics_for_tool(ctx, &input, tool_name, root),
            changed.as_ref(),
        );

        let view = build_view(&input, tool_name, root, &hints, &diagnostics);
        let rendered = self.engine.render(TMPL_POST, &view);
        let trimmed = rendered.trim();

        if trimmed.is_empty() {
            Ok(HookOutput::empty())
        } else {
            Ok(HookOutput::context("PostToolUse", trimmed.to_owned()).to_bytes())
        }
    }
}

/// Build the template view from raw hook input + derived fields.
fn build_view(
    input: &HookInput,
    tool_name: &str,
    root: &str,
    hints: &[HintView],
    diagnostics: &[DiagnosticRow],
) -> minijinja::Value {
    let tool_input = input.tool_input.clone().unwrap_or(serde_json::Value::Null);

    // Deserialize typed inputs once for reuse across sections.
    let bash_input = (tool_name == "Bash")
        .then(|| input.tool_input_as::<BashToolInput>())
        .flatten();
    let bash_cmd = bash_input.and_then(|b| b.command);

    // Bash: extract command name and relative file paths.
    let (bin, rel_paths) = match &bash_cmd {
        Some(cmd) => (Some(extract_command_name(cmd)), extract_rel_paths(cmd, root)),
        None => (None, Vec::new()),
    };

    // Edit/Write: file path, relative path, symbol, VFS status.
    let file_path = tool_file_path(input, tool_name);
    let is_vfs = file_path.as_deref().is_some_and(super::is_vfs_path);
    let rel = file_path
        .as_deref()
        .map(|fp| if is_vfs { super::source_file_of(fp) } else { fp })
        .and_then(|src| src.strip_prefix(root))
        .map(str::to_owned);
    let sym = file_path
        .as_deref()
        .and_then(super::symbol_from_vfs_path)
        .map(str::to_owned);

    // SSOT: only on significant edits or any write.
    let ssot = match tool_name {
        "Edit" =>
            if let Some(edit) = input.tool_input_as::<EditToolInput>() {
                let old = edit.old_string.as_deref().map_or(0, |s| s.lines().count());
                let new = edit.new_string.as_deref().map_or(0, |s| s.lines().count());
                old + new > SSOT_LINE_THRESHOLD
            } else {
                false
            },
        "Write" => true,
        _ => false,
    };

    // Overview read hint (Read tool or `cat` bash command).
    let overview_rel = match tool_name {
        "Read" => input
            .tool_input_as::<ReadToolInput>()
            .and_then(|r| r.file_path)
            .filter(|fp| super::is_symbols_overview(fp))
            .map(|fp| strip_to_rel(&fp, root)),
        "Bash" => bash_cmd
            .as_deref()
            .and_then(extract_cat_overview_path)
            .filter(|fp| super::is_symbols_overview(fp))
            .map(|fp| strip_to_rel(&fp, root)),
        _ => None,
    };

    minijinja::context! {
        tool_name,
        tool_input,
        bin,
        rel_paths,
        rel,
        sym,
        is_vfs,
        ssot,
        overview_rel,
        hints,
        diagnostics,
    }
}

/// Strip a VFS path to its relative source file form.
fn strip_to_rel(fp: &str, root: &str) -> String {
    let src = super::source_file_of(fp);
    src.strip_prefix(root).unwrap_or(src).to_owned()
}

/// Extract the base command name from a shell command string, skipping env
/// vars, cd prefixes, and path components.
fn extract_command_name(cmd: &str) -> String {
    let mut tokens = cmd.split_whitespace().peekable();

    // Skip variable assignments (FOO=bar) and `env` prefix.
    while let Some(&tok) = tokens.peek() {
        if tok == "env" || tok.contains('=') {
            tokens.next();
        } else {
            break;
        }
    }

    // Skip cd prefix (cd <path> && ...)
    if tokens.peek() == Some(&"cd") {
        for tok in tokens.by_ref() {
            if tok == "&&" || tok == ";" {
                break;
            }
        }
    }

    tokens
        .next()
        .and_then(|s| s.rsplit('/').next())
        .unwrap_or("")
        .to_owned()
}

/// Extract relative paths from a command string — tokens under root that
/// aren't VFS virtual paths.
fn extract_rel_paths(cmd: &str, root_str: &str) -> Vec<String> {
    cmd.split_whitespace()
        .filter(|tok| tok.starts_with(root_str) && !super::is_vfs_path(tok))
        .filter_map(|tok| {
            let cleaned = tok.trim_end_matches([',', ':', ';']);
            cleaned.strip_prefix(root_str).map(str::to_owned)
        })
        .collect()
}

/// Extract an OVERVIEW.md path from a `cat <path>` command.
fn extract_cat_overview_path(cmd: &str) -> Option<String> {
    let cat_pos = cmd.find("cat ")?;
    let after_cat = cmd[cat_pos + 4..].trim_start();
    let path = after_cat.split_whitespace().next()?;
    path.ends_with(FILE_OVERVIEW).then(|| path.to_owned())
}

/// Extract the file path from an Edit or Write tool call.
fn tool_file_path(input: &HookInput, tool_name: &str) -> Option<String> {
    match tool_name {
        "Edit" => input.tool_input_as::<EditToolInput>().and_then(|e| e.file_path),
        "Write" => input.tool_input_as::<WriteToolInput>().and_then(|w| w.file_path),
        _ => None,
    }
}
/// Extract the source file's relative path from an Edit/Write tool call.
///
/// Returns `None` for non-file tools or paths outside root.
/// VFS paths are resolved to their underlying source file.
fn source_rel_path(input: &HookInput, tool_name: &str, root: &str) -> Option<String> {
    let file_path = tool_file_path(input, tool_name)?;

    let src = if super::is_vfs_path(&file_path) {
        super::source_file_of(&file_path)
    } else {
        &file_path
    };

    src.strip_prefix(root).map(str::to_owned)
}

/// Unit tests.
#[cfg(test)]
mod tests;

/// Analysis results: hints plus the decomposed source for change-range filtering.
struct AnalysisResult {
    hints: Vec<HintView>,
    decomposed: Option<Arc<DecomposedSource>>,
}

/// Run syntax analysis on the file targeted by an Edit/Write tool call.
///
/// Returns hints and the decomposed source (used by the caller to compute
/// the changed line range for filtering). Returns empty hints for non-file
/// tools or files without tree-sitter support.
fn run_analysis_for_tool(
    engine: &AnalysisEngine,
    ctx: &ScriptContext<'_>,
    input: &HookInput,
    tool_name: &str,
    root: &str,
) -> AnalysisResult {
    let empty = AnalysisResult {
        hints: Vec::new(),
        decomposed: None,
    };

    let Some(rel) = source_rel_path(input, tool_name, root) else {
        return empty;
    };

    let Ok(vfs_path) = VfsPath::new(&rel) else {
        return empty;
    };

    let services = CodingServices::get(ctx.activation());

    // The Edit/Write tool writes directly to the real filesystem, bypassing
    // the FUSE mount. The inotify watcher will eventually invalidate, but
    // it's async — by the time this hook runs the cache still holds the
    // pre-edit parse tree. Invalidate explicitly so we analyze fresh content.
    services.decomposition.invalidate(&vfs_path);

    let Ok(decomposed) = services.decomposition.get(&vfs_path) else {
        return empty;
    };

    let Some(tree) = &decomposed.tree else {
        return AnalysisResult {
            hints: Vec::new(),
            decomposed: Some(decomposed),
        };
    };

    let analysis_ctx = AnalysisContext {
        source: &decomposed.source,
        activation: ctx.activation(),
    };

    let hints = engine.analyze(tree, &analysis_ctx).iter().map(HintView::from).collect();
    AnalysisResult {
        hints,
        decomposed: Some(decomposed),
    }
}

/// Fetch LSP diagnostics for the file targeted by an Edit/Write tool call.
///
/// Returns an empty vec for non-file tools or files without LSP support.
/// VFS paths are resolved to their underlying source file. Blocks up to
/// `diagnostics_timeout` if the file was recently changed (waiting for the
/// server to push fresh diagnostics).
fn fetch_diagnostics_for_tool(
    ctx: &ScriptContext<'_>,
    input: &HookInput,
    tool_name: &str,
    root: &str,
) -> Vec<DiagnosticRow> {
    let Some(rel) = source_rel_path(input, tool_name, root) else {
        return Vec::new();
    };

    let ext = Path::new(&rel).extension().and_then(|e| e.to_str());
    let Some(ext) = ext else {
        return Vec::new();
    };

    let services = CodingServices::get(ctx.activation());

    let lsp_file = ctx.activation().overlay_root().join(&rel);
    services.lsp.ensure_document_open(&lsp_file, ext);

    // The FUSE write handler writes through to the overlay, but LSP
    // invalidation is triggered by the inotify watcher on the overlay —
    // which is async. By the time this hook runs, the watcher may not
    // have fired yet, so the LSP server still has stale content and the
    // DiagnosticStore isn't marked dirty. Explicitly invalidate to send
    // `didChange` now and mark the store dirty for the blocking wait.
    services.lsp.invalidate_file(&lsp_file);

    let Some(fq) = services.lsp.file_query(&lsp_file, ext) else {
        return Vec::new();
    };

    let diags = fq.diagnostics().unwrap_or_default();
    diagnostics_to_rows(&diags)
}
/// Compute the 1-based line range affected by an Edit tool call.
///
/// Finds `new_string` in the post-edit source and expands to the enclosing
/// tree-sitter scope. Returns `None` for Write (entire file changed),
/// `replace_all` edits, empty replacements, or when the edit location is
/// ambiguous (multiple matches).
fn changed_line_range(input: &HookInput, tool_name: &str, decomposed: &DecomposedSource) -> Option<Range<usize>> {
    if tool_name != "Edit" {
        return None;
    }

    let edit = input.tool_input_as::<EditToolInput>()?;
    let new_string = edit.new_string.as_deref()?;

    // Pure deletion — can't locate in post-edit source.
    if new_string.is_empty() {
        return None;
    }

    // replace_all scatters changes across the file.
    if edit.replace_all == Some(true) {
        return None;
    }

    // Require a unique match to avoid filtering to the wrong location.
    let matches: Vec<usize> = decomposed
        .source
        .match_indices(new_string)
        .map(|(idx, _)| idx)
        .collect();
    let &[byte_start] = matches.as_slice() else {
        return None;
    };
    let byte_end = byte_start + new_string.len();

    // 0-based line numbers via Rope (O(log n) instead of O(n) byte scan).
    let rope = crop::Rope::from(&*decomposed.source);
    let start_line = rope.line_of_byte(byte_start);
    let end_line = rope.line_of_byte(byte_end);

    // Expand to enclosing tree-sitter scope if a parse tree is available.
    let (scope_start, scope_end) = decomposed
        .tree
        .as_ref()
        .and_then(|tree| enclosing_scope_lines(tree, byte_start, byte_end))
        .unwrap_or((start_line, end_line));

    // Convert to 1-based to match HintView.line_start and DiagnosticRow.line.
    Some((scope_start + 1)..(scope_end + 2))
}

/// Find the enclosing scope node's 0-based line range for a byte span.
///
/// Walks up from the deepest node containing the byte range, stopping at
/// the first named ancestor with a meaningful span (≥ 5 lines or the change
/// size). This naturally lands on function/method/class boundaries.
fn enclosing_scope_lines(tree: &tree_sitter::Tree, byte_start: usize, byte_end: usize) -> Option<(usize, usize)> {
    let root = tree.root_node();
    let mut node = root.descendant_for_byte_range(byte_start, byte_end.saturating_sub(1))?;

    let raw_lines = node.end_position().row - node.start_position().row + 1;
    let min_scope = raw_lines.max(5);

    loop {
        let span = node.end_position().row - node.start_position().row + 1;

        if node.id() == root.id() {
            // Reached root — don't filter to the whole file.
            return None;
        }

        if node.is_named() && span >= min_scope {
            return Some((node.start_position().row, node.end_position().row));
        }

        node = node.parent()?;
    }
}

/// Filter hints to those overlapping the changed line range.
fn filter_hints(hints: Vec<HintView>, changed: Option<&Range<usize>>) -> Vec<HintView> {
    let Some(range) = changed else {
        return hints;
    };
    hints
        .into_iter()
        .filter(|h| h.line_start < range.end && h.line_end >= range.start)
        .collect()
}

/// Filter diagnostics to those within the changed line range.
fn filter_diagnostics(diagnostics: Vec<DiagnosticRow>, changed: Option<&Range<usize>>) -> Vec<DiagnosticRow> {
    let Some(range) = changed else {
        return diagnostics;
    };
    diagnostics
        .into_iter()
        .filter(|d| (d.line as usize) >= range.start && (d.line as usize) < range.end)
        .collect()
}
