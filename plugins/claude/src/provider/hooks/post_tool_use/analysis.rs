//! All analysis-feature-gated code consolidated behind a single `#[cfg]` boundary.
//!
//! This module contains types and helpers that depend on `nyne_analysis` (`HintView`,
//! `Engine`, etc.) and only exist when the `analysis` feature is enabled.
//! The public entry point is [`run_analysis`], re-exported into the parent module.

use nyne_analysis::{Engine, HintView};

use super::{
    Arc, DecomposedSource, HookInput, Range, ScriptContext, Services, VfsPath, changed_line_range, source_rel_path,
};
use crate::provider::hook_schema::EditToolInput;

/// Per-file analysis output: hints plus the decomposed source for change-range filtering.
struct FileAnalysis {
    hints: Vec<HintView>,
    decomposed: Option<Arc<DecomposedSource>>,
}

/// Run syntax analysis on the file targeted by an Edit/Write tool call.
///
/// Returns hints and the decomposed source (used by the caller to compute
/// the changed line range for filtering). Returns empty hints for non-file
/// tools or files without tree-sitter support.
fn run_analysis_for_tool(
    ctx: &ScriptContext<'_>,
    edit_input: Option<&EditToolInput>,
    input: &HookInput,
    tool_name: &str,
    root: &str,
) -> FileAnalysis {
    let empty = FileAnalysis {
        hints: Vec::new(),
        decomposed: None,
    };

    let Some(rel) = source_rel_path(edit_input, input, tool_name, root) else {
        return empty;
    };

    let Ok(vfs_path) = VfsPath::new(&rel) else {
        return empty;
    };

    let services = Services::get(ctx.activation());

    // The Edit/Write tool writes directly to the real filesystem, bypassing
    // the FUSE mount. The inotify watcher will eventually invalidate, but
    // it's async — by the time this hook runs the cache still holds the
    // pre-edit parse tree. Invalidate explicitly so we analyze fresh content.
    services.decomposition.invalidate(&vfs_path);

    let Ok(decomposed) = services.decomposition.get(&vfs_path) else {
        return empty;
    };

    let Some(tree) = &decomposed.tree else {
        return FileAnalysis {
            decomposed: Some(decomposed),
            ..empty
        };
    };

    let Some(engine) = ctx.activation().get::<Arc<Engine>>() else {
        return FileAnalysis {
            decomposed: Some(decomposed),
            ..empty
        };
    };

    FileAnalysis {
        hints: engine
            .analyze(tree, &decomposed.source)
            .iter()
            .map(HintView::from)
            .collect(),
        decomposed: Some(decomposed),
    }
}

/// Filter hints to those overlapping the changed line range.
pub(super) fn filter_hints(hints: Vec<HintView>, changed: Option<&Range<usize>>) -> Vec<HintView> {
    let Some(range) = changed else {
        return hints;
    };
    hints
        .into_iter()
        .filter(|h| h.line_start < range.end && h.line_end >= range.start)
        .collect()
}

/// Run analysis and compute filtered hints plus the changed line range.
///
/// When the `analysis` feature is disabled, a stub returning empty defaults
/// is used instead (see `mod.rs`).
pub(super) fn run_analysis(
    ctx: &ScriptContext<'_>,
    edit_input: Option<&EditToolInput>,
    input: &HookInput,
    tool_name: &str,
    root: &str,
) -> (Vec<HintView>, Option<Range<usize>>) {
    let result = run_analysis_for_tool(ctx, edit_input, input, tool_name, root);
    let changed = result
        .decomposed
        .as_deref()
        .and_then(|d| changed_line_range(edit_input, d));
    let hints = filter_hints(result.hints, changed.as_ref());
    (hints, changed)
}
