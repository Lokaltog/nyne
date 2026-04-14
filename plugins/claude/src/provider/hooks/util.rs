//! Shared helpers for hook scripts.
//!
//! Every hook script is a narrow, single-concern implementation
//! of [`Script`](nyne::Script). They share a small set of input-parsing
//! and path-resolution helpers that live here to keep the individual
//! script modules focused.

use std::ops::Range;
use std::path::Path;
use std::time::SystemTime;

use nyne::router::{Chain, Filesystem};
use nyne::templates::TemplateEngine;
use nyne::{ActivationContext, ScriptContext};
#[cfg(feature = "lsp")]
use nyne_lsp::LspContextExt as _;
#[cfg(feature = "lsp")]
use nyne_lsp::{DiagnosticRow, diagnostics_to_rows};
use nyne_source::{DecomposedSource, Fragment, find_fragment_at_line};

use crate::provider::hook_schema::{EditToolInput, HookInput, ReadToolInput, ToolKind, WriteToolInput};
use crate::provider::settings::RAW_FILE_GRACE_SECS;

/// Extract the base command name from a shell command string, skipping env
/// vars, cd prefixes, and path components.
pub(super) fn extract_command_name(cmd: &str) -> String {
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
pub(super) fn extract_rel_paths(cmd: &str, root_str: &str, chain: &Chain) -> Vec<String> {
    cmd.split_whitespace()
        .filter(|tok| tok.starts_with(root_str) && super::resolve_companion(chain, root_str, tok).is_none())
        .filter_map(|tok| {
            tok.trim_end_matches([',', ':', ';'])
                .strip_prefix(root_str)
                .map(str::to_owned)
        })
        .collect()
}

/// Extract the file path from a Read, Edit, or Write tool call.
///
/// `Edit` reuses the pre-parsed `edit_input` from the caller (common
/// because the caller also needs `old_string`/`new_string`). `Read`
/// and `Write` parse their typed input here on demand.
pub(super) fn tool_file_path(edit_input: Option<&EditToolInput>, input: &HookInput, kind: ToolKind) -> Option<String> {
    match kind {
        ToolKind::Read => input.tool_input_as::<ReadToolInput>().and_then(|r| r.file_path),
        ToolKind::Edit => edit_input.and_then(|e| e.file_path.clone()),
        ToolKind::Write => input.tool_input_as::<WriteToolInput>().and_then(|w| w.file_path),
    }
}

/// Extract the source file's relative path from a Read/Edit/Write tool call.
///
/// Returns `None` for non-file tools or paths outside root. VFS paths are
/// resolved to their underlying source file via pipeline evaluation.
pub(super) fn source_rel_path(
    edit_input: Option<&EditToolInput>,
    input: &HookInput,
    kind: ToolKind,
    root: &str,
    chain: &Chain,
) -> Option<String> {
    let file_path = tool_file_path(edit_input, input, kind)?;

    match super::resolve_companion(chain, root, &file_path) {
        Some(c) => c.source_file.and_then(|sf| sf.to_str().map(str::to_owned)),
        None => file_path.strip_prefix(root).map(str::to_owned),
    }
}

/// Fetch LSP diagnostics for a pre-resolved source file path.
///
/// Returns an empty vec for files without an extension or without LSP
/// support. The caller is responsible for resolving VFS paths to their
/// underlying source file via [`source_rel_path`]; this function takes
/// the resolved `rel` directly to avoid duplicating the resolution work
/// when a single `exec` call needs both analysis and diagnostics for
/// the same file.
#[cfg(feature = "lsp")]
pub(super) fn fetch_diagnostics_for_tool(ctx: &ScriptContext<'_>, rel: &str) -> Vec<DiagnosticRow> {
    let Some(ext) = Path::new(rel).extension().and_then(|e| e.to_str()) else {
        return Vec::new();
    };

    let Some(lsp) = ctx.activation().lsp_manager() else {
        return Vec::new();
    };

    let lsp_file = ctx.activation().source_path(rel);
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
pub(super) fn changed_line_range(edit: Option<&EditToolInput>, decomposed: &DecomposedSource) -> Option<Range<usize>> {
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
pub(super) fn filter_diagnostics(
    diagnostics: Vec<DiagnosticRow>,
    changed: Option<&Range<usize>>,
) -> Vec<DiagnosticRow> {
    let Some(range) = changed else {
        return diagnostics;
    };
    diagnostics
        .into_iter()
        .filter(|d| (d.line as usize) >= range.start && (d.line as usize) < range.end)
        .collect()
}

/// Check if the file's atime is within the grace period.
///
/// After denying a raw file read, the hook stamps the file's atime via
/// [`stamp_atime`]. Subsequent accesses within [`RAW_FILE_GRACE_SECS`]
/// are allowed through without a hint/deny, preventing an annoying
/// loop when the agent retries immediately after being redirected.
pub(super) fn is_within_grace(fs: &dyn Filesystem, rel: &str) -> bool {
    let Ok(meta) = fs.metadata(Path::new(rel)) else {
        return false;
    };
    let Ok(elapsed) = SystemTime::now().duration_since(meta.timestamps.atime) else {
        return false;
    };
    elapsed.as_secs() < RAW_FILE_GRACE_SECS
}

/// Stamp atime to suppress re-triggering within the grace period.
pub(super) fn stamp_atime(activation: &ActivationContext, rel: &str) {
    let _ = filetime::set_file_atime(activation.source_path(rel), filetime::FileTime::now());
}

/// Find the 1-based line number of the first occurrence of `needle` in
/// an in-memory source string.
///
/// Operates on the cached [`DecomposedSource::source`] that the caller
/// already holds via the decomposition cache — no disk re-read.
pub(super) fn find_line_of_string(source: &str, needle: &str) -> Option<u64> {
    let first_line = needle.lines().next()?;
    source
        .lines()
        .position(|line| line.contains(first_line))
        .and_then(|i| u64::try_from(i + 1).ok())
}

/// Resolve a 0-based line number to a symbol name path.
///
/// Returns fragment path segments joined with `/` (e.g., `Foo/bar`).
/// TODO: use `SourceContext` from pipeline state once available to build
/// companion-suffixed VFS display paths.
pub(super) fn resolve_symbol_at_line(fragments: &[Fragment], line: usize, rope: &crop::Rope) -> Option<String> {
    Some(find_fragment_at_line(fragments, line, rope)?.join("/"))
}

/// Shared scaffolding for hook scripts — parse stdin, run the
/// per-script view builder, and render the template (or emit an empty
/// output when the builder returns `None`).
///
/// Every `Script::exec` boils down to the same sequence:
///
/// 1. Parse the JSON payload into [`HookInput`].
/// 2. Build a template view from the parsed input (per-script logic).
/// 3. If the view is `None`, return empty bytes; otherwise render the
///    template and wrap the output as a `HookOutput::context` message.
///
/// This helper collapses that boilerplate to one line per script. The
/// `build` closure receives the parsed `HookInput` and the
/// `ScriptContext`; it returns the rendering context (any `Serialize`)
/// or `None` to signal a no-op. The `event_name` is one of
/// `"PreToolUse"` / `"PostToolUse"` — passed through to
/// [`HookOutput::context`](crate::provider::hook_schema::HookOutput::context).
pub(super) fn run_script<V, F>(
    ctx: &ScriptContext<'_>,
    stdin: &[u8],
    engine: &TemplateEngine,
    tmpl: &str,
    event_name: &'static str,
    build: F,
) -> Vec<u8>
where
    V: serde::Serialize,
    F: FnOnce(&HookInput, &ScriptContext<'_>) -> Option<V>,
{
    use crate::provider::hook_schema::HookOutput;

    let Some(input) = HookInput::parse(stdin) else {
        return HookOutput::empty();
    };
    let Some(view) = build(&input, ctx) else {
        return HookOutput::empty();
    };
    super::render_context(engine, tmpl, &view, event_name)
}
