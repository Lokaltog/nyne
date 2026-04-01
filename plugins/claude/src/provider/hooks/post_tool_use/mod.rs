//! `PostToolUse` hook — fires after tool execution completes.
//!
//! All rendering logic lives in `templates/post-tool-use.md.j2`.
//! This module computes derived fields that Jinja can't (command parsing,
//! VFS path extraction, line counting) and passes them alongside the raw
//! `tool_input` to the template.

use std::ops::Range;
use std::path::Path;

use nyne::prelude::*;
use nyne::router::Chain;
use nyne::templates::TemplateEngine;
use nyne::{Script, ScriptContext};
#[cfg(feature = "lsp")]
use nyne_lsp::LspContextExt as _;
#[cfg(feature = "lsp")]
use nyne_lsp::{DiagnosticRow, diagnostics_to_rows};
use nyne_source::DecomposedSource;

use crate::provider::hook_schema::{
    BashToolInput, EditToolInput, HookInput, HookOutput, ReadToolInput, WriteToolInput,
};

/// Minimum combined old+new line count to trigger SSOT reminder on Edit.
///
/// When an Edit tool call's `old_string` + `new_string` combined line count
/// exceeds this threshold, the post-hook emits an SSOT/DRY check reminder.
/// Small edits (renames, one-liners) are not worth the noise.
const SSOT_LINE_THRESHOLD: usize = 10;

/// Template key for the post-tool-use hook.
const TMPL_POST: &str = "claude/post-tool-use";

/// `PostToolUse` hook script implementation.
///
/// Fires after every tool execution. For Edit and Write tools targeting
/// source files, runs syntax analysis and fetches LSP diagnostics scoped
/// to the changed line range. For Bash commands, extracts relative paths
/// to provide VFS navigation hints. All derived data is passed to a
/// Jinja template that decides what context to surface.
pub(in crate::provider) struct PostToolUse {
    pub(in crate::provider) engine: Arc<TemplateEngine>,
}

pub(in crate::provider) fn build_engine() -> Arc<TemplateEngine> {
    let mut b = super::hook_builder();
    b.register(TMPL_POST, include_str!("../templates/post-tool-use.md.j2"));
    b.finish()
}
/// [`Script`] implementation for [`PostToolUse`].
impl Script for PostToolUse {
    /// Process post-tool-use hook input and render analysis context.
    fn exec(&self, ctx: &ScriptContext<'_>, stdin: &[u8]) -> Result<Vec<u8>> {
        let Some(input) = HookInput::parse(stdin) else {
            return Ok(HookOutput::empty());
        };
        let tool_name = input.tool_name.clone().unwrap_or_default();
        let root = ctx.activation().root_prefix();
        let chain = ctx.chain();

        // Deserialize EditToolInput once — reused by analysis, diagnostics, and view.
        let edit_input = (tool_name == "Edit")
            .then(|| input.tool_input_as::<EditToolInput>())
            .flatten();

        let (analysis, changed) = run_analysis(ctx, edit_input.as_ref(), &input, &tool_name, root);

        #[cfg(feature = "lsp")]
        let diagnostics = filter_diagnostics(
            fetch_diagnostics_for_tool(ctx, edit_input.as_ref(), &input, &tool_name, root),
            changed.as_ref(),
        );
        #[cfg(not(feature = "lsp"))]
        let diagnostics: Vec<minijinja::Value> = Vec::new();

        let _ = &changed; // suppress unused warning when lsp is disabled

        let results = ToolResults { analysis, diagnostics };
        let view = build_view(input, edit_input.as_ref(), &tool_name, root, chain, &results);
        Ok(super::render_context(&self.engine, TMPL_POST, &view, "PostToolUse"))
    }
}

/// Bundled analysis + diagnostics results for template rendering.
struct ToolResults<A, D> {
    analysis: A,
    diagnostics: D,
}

/// Build the template view from raw hook input + derived fields.
fn build_view(
    input: HookInput,
    edit_input: Option<&EditToolInput>,
    tool_name: &str,
    root: &str,
    chain: &Chain,
    results: &ToolResults<impl serde::Serialize, impl serde::Serialize>,
) -> minijinja::Value {
    // Deserialize typed inputs before consuming `input.tool_input`.
    let bash_cmd = (tool_name == "Bash")
        .then(|| input.tool_input_as::<BashToolInput>())
        .flatten()
        .and_then(|b| b.command);

    // Overview read hint (Read tool or `cat` bash command).
    let overview_rel = match tool_name {
        "Read" => input
            .tool_input_as::<ReadToolInput>()
            .and_then(|r| r.file_path)
            .filter(|fp| super::is_symbols_overview(fp))
            .map(|fp| strip_to_rel(&fp, root, chain)),
        "Bash" => bash_cmd
            .as_deref()
            .and_then(extract_cat_overview_path)
            .filter(|fp| super::is_symbols_overview(fp))
            .map(|fp| strip_to_rel(&fp, root, chain)),
        _ => None,
    };

    // Edit/Write: file path, relative path, symbol, VFS status.
    let file_path = tool_file_path(edit_input, &input, tool_name);
    let companion = file_path
        .as_deref()
        .and_then(|fp| super::resolve_companion(chain, root, fp));
    let is_vfs = companion.is_some();
    let rel = match &companion {
        Some(c) => c.source_file.as_ref().and_then(|sf| sf.to_str()).map(str::to_owned),
        None => file_path
            .as_deref()
            .and_then(|fp| fp.strip_prefix(root))
            .map(str::to_owned),
    };
    // TODO: derive sym from pipeline state once the source plugin sets
    // a ResolvedFragment (or similar) on the request during dispatch.
    let sym: Option<String> = None;

    // Consume tool_input — all typed deserialization is done above.
    let tool_input = input.tool_input.unwrap_or(serde_json::Value::Null);

    // Bash: extract command name and relative file paths.
    let (bin, rel_paths) = bash_cmd.as_deref().map_or((None, Vec::new()), |cmd| {
        (Some(extract_command_name(cmd)), extract_rel_paths(cmd, root, chain))
    });

    // SSOT: only on significant edits or any write.
    let ssot = match tool_name {
        "Edit" => edit_input.is_some_and(|edit| {
            let old = edit.old_string.as_deref().map_or(0, |s| s.lines().count());
            let new = edit.new_string.as_deref().map_or(0, |s| s.lines().count());
            old + new > SSOT_LINE_THRESHOLD
        }),
        "Write" => true,
        _ => false,
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
        analysis => results.analysis,
        diagnostics => results.diagnostics,
    }
}

/// Strip a path to its relative source file form.
///
/// VFS paths are resolved to their underlying source file via pipeline evaluation.
fn strip_to_rel(fp: &str, root: &str, chain: &Chain) -> String {
    super::resolve_companion(chain, root, fp)
        .and_then(|c| c.source_file)
        .and_then(|sf| sf.to_str().map(str::to_owned))
        .unwrap_or_else(|| fp.strip_prefix(root).unwrap_or(fp).to_owned())
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
fn extract_rel_paths(cmd: &str, root_str: &str, chain: &Chain) -> Vec<String> {
    cmd.split_whitespace()
        .filter(|tok| tok.starts_with(root_str) && super::resolve_companion(chain, root_str, tok).is_none())
        .filter_map(|tok| {
            let cleaned = tok.trim_end_matches([',', ':', ';']);
            cleaned.strip_prefix(root_str).map(str::to_owned)
        })
        .collect()
}

/// Extract an OVERVIEW.md path from a `cat <path>` command.
fn extract_cat_overview_path(cmd: &str) -> Option<String> {
    let path = cmd[cmd.find("cat ")? + 4..].split_whitespace().next()?;
    path.ends_with("OVERVIEW.md").then(|| path.to_owned())
}

/// Extract the file path from an Edit or Write tool call.
fn tool_file_path(edit_input: Option<&EditToolInput>, input: &HookInput, tool_name: &str) -> Option<String> {
    match tool_name {
        "Edit" => edit_input.and_then(|e| e.file_path.clone()),
        "Write" => input.tool_input_as::<WriteToolInput>().and_then(|w| w.file_path),
        _ => None,
    }
}

/// Extract the source file's relative path from an Edit/Write tool call.
///
/// Returns `None` for non-file tools or paths outside root.
/// VFS paths are resolved to their underlying source file via pipeline evaluation.
fn source_rel_path(
    edit_input: Option<&EditToolInput>,
    input: &HookInput,
    tool_name: &str,
    root: &str,
    chain: &Chain,
) -> Option<String> {
    let file_path = tool_file_path(edit_input, input, tool_name)?;

    match super::resolve_companion(chain, root, &file_path) {
        Some(c) => c.source_file.and_then(|sf| sf.to_str().map(str::to_owned)),
        None => file_path.strip_prefix(root).map(str::to_owned),
    }
}

/// Unit tests.
#[cfg(test)]
mod tests;

/// Fetch LSP diagnostics for the file targeted by an Edit/Write tool call.
///
/// Returns an empty vec for non-file tools or files without LSP support.
/// VFS paths are resolved to their underlying source file. Blocks up to
/// `diagnostics_timeout` if the file was recently changed (waiting for the
/// server to push fresh diagnostics).
#[cfg(feature = "lsp")]
fn fetch_diagnostics_for_tool(
    ctx: &ScriptContext<'_>,
    edit_input: Option<&EditToolInput>,
    input: &HookInput,
    tool_name: &str,
    root: &str,
) -> Vec<DiagnosticRow> {
    let Some(rel) = source_rel_path(edit_input, input, tool_name, root, ctx.chain()) else {
        return Vec::new();
    };

    let ext = Path::new(&rel).extension().and_then(|e| e.to_str());
    let Some(ext) = ext else {
        return Vec::new();
    };

    let Some(lsp) = ctx.activation().lsp_manager() else {
        return Vec::new();
    };

    let lsp_file = ctx.activation().source_path(&rel);
    lsp.ensure_document_open(&lsp_file, ext);

    // The FUSE write handler writes through to the source root, but LSP
    // invalidation is triggered by the inotify watcher on the source root —
    // which is async. By the time this hook runs, the watcher may not
    // have fired yet, so the LSP server still has stale content and the
    // DiagnosticStore isn't marked dirty. Explicitly invalidate to send
    // `didChange` now and mark the store dirty for the blocking wait.
    lsp.invalidate_file(&lsp_file);

    let Some(fq) = lsp.file_query(&lsp_file, ext) else {
        return Vec::new();
    };

    diagnostics_to_rows(&fq.diagnostics().unwrap_or_default())
}

/// Compute the 1-based line range affected by an Edit tool call.
///
/// Finds `new_string` in the post-edit source and expands to the enclosing
/// tree-sitter scope. Returns `None` for non-Edit tools, `replace_all` edits,
/// empty replacements, or when the edit location is ambiguous (multiple matches).
fn changed_line_range(edit: Option<&EditToolInput>, decomposed: &DecomposedSource) -> Option<Range<usize>> {
    let edit = edit?;
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

    // Convert to 1-based to match DiagnosticRow.line (and HintView.line_start
    // when the analysis feature is enabled).
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

/// Filter diagnostics to those within the changed line range.
#[cfg(feature = "lsp")]
fn filter_diagnostics(diagnostics: Vec<DiagnosticRow>, changed: Option<&Range<usize>>) -> Vec<DiagnosticRow> {
    let Some(range) = changed else {
        return diagnostics;
    };
    diagnostics
        .into_iter()
        .filter(|d| (d.line as usize) >= range.start && (d.line as usize) < range.end)
        .collect()
}

#[cfg(feature = "analysis")]
mod analysis;

#[cfg(feature = "analysis")]
use analysis::run_analysis;

/// Stub for when the `analysis` feature is disabled — returns empty defaults.
#[cfg(not(feature = "analysis"))]
fn run_analysis(
    _ctx: &ScriptContext<'_>,
    _edit_input: Option<&EditToolInput>,
    _input: &HookInput,
    _tool_name: &str,
    _root: &str,
) -> (Vec<()>, Option<Range<usize>>) {
    (Vec::new(), None)
}
