//! All analysis-feature-gated code consolidated behind a single `#[cfg]` boundary.
//!
//! This module contains types and helpers that depend on `nyne_analysis` (`HintView`,
//! `Engine`, etc.) and only exist when the `analysis` feature is enabled.
//! The public entry point is [`run_analysis`], re-exported into the parent module.

use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use nyne::ScriptContext;
use nyne_analysis::{AnalysisContextExt as _, HintView};
use nyne_source::{DecomposedSource, SourceContextExt as _};

use super::super::util::changed_line_range;
use crate::provider::hook_schema::EditToolInput;

/// Per-file analysis output: hints plus the decomposed source for change-range filtering.
struct FileAnalysis {
    hints: Vec<HintView>,
    decomposed: Option<Arc<DecomposedSource>>,
}

/// Run syntax analysis on a pre-resolved source file path.
///
/// Returns hints and the decomposed source (used by the caller to compute
/// the changed line range for filtering). Returns empty for files without
/// tree-sitter support.
fn run_analysis_for_tool(ctx: &ScriptContext<'_>, rel: &str) -> FileAnalysis {
    let empty = FileAnalysis {
        hints: Vec::new(),
        decomposed: None,
    };

    let rel_path = Path::new(rel);
    let Some(cache) = ctx.activation().decomposition_cache() else {
        return empty;
    };

    // The Edit/Write tool writes directly to the real filesystem, bypassing
    // the FUSE mount. The inotify watcher will eventually invalidate, but
    // it's async — by the time this hook runs the cache still holds the
    // pre-edit parse tree. Invalidate explicitly so we analyze fresh content.
    cache.invalidate(rel_path);

    let Ok(decomposed) = cache.get(rel_path) else {
        return empty;
    };

    let Some(tree) = &decomposed.tree else {
        return FileAnalysis {
            decomposed: Some(decomposed),
            ..empty
        };
    };

    let Some(engine) = ctx.activation().analysis_engine() else {
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
/// is used instead (see `diagnostics.rs`).
pub(super) fn run_analysis(
    ctx: &ScriptContext<'_>,
    edit_input: Option<&EditToolInput>,
    rel: &str,
) -> (Vec<HintView>, Option<Range<usize>>) {
    let result = run_analysis_for_tool(ctx, rel);
    let changed = result
        .decomposed
        .as_deref()
        .and_then(|d| changed_line_range(edit_input, d));
    let hints = filter_hints(result.hints, changed.as_ref());
    (hints, changed)
}
