//! Analysis rule: detect unwrap chains.

use super::kinds;
use crate::syntax::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};
use crate::syntax::parser::TsNode;

/// Minimum `.unwrap()` calls to trigger (in a single statement or consecutive).
const MIN_UNWRAPS: usize = 2;

/// Analysis rule that detects chained unwrap calls.
struct UnwrapChain;

/// [`AnalysisRule`] implementation for `UnwrapChain`.
impl AnalysisRule for UnwrapChain {
    fn id(&self) -> &'static str { "unwrap-chain" }

    /// Trigger on expression statements — we scan each for chained unwraps.
    fn node_kinds(&self) -> &'static [&'static str] { &[kinds::EXPRESSION_STATEMENT, "let_declaration"] }

    fn check(&self, node: TsNode<'_>, _context: &AnalysisContext<'_>) -> Option<Hint> {
        let raw = node.raw();
        let source = node.source();
        let count = count_unwrap_calls(raw, source);

        if count < MIN_UNWRAPS {
            return None;
        }

        let line = raw.start_position().row;

        Some(Hint {
            rule_id: self.id(),
            severity: Severity::Warning,
            line_range: line..line,
            message: format!("{count} `.unwrap()` calls in one statement"),
            suggestions: vec!["Propagate with `?` or use `let...else`".into()],
        })
    }
}

/// Count the number of .`unwrap()` calls within a node subtree.
fn count_unwrap_calls(node: tree_sitter::Node<'_>, source: &[u8]) -> usize {
    let mut count = 0;
    let mut cursor = node.walk();
    count_unwraps_recursive(&mut cursor, source, &mut count);
    count
}

/// Recursively walk a subtree counting .`unwrap()` call nodes.
fn count_unwraps_recursive(cursor: &mut tree_sitter::TreeCursor<'_>, source: &[u8], count: &mut usize) {
    let node = cursor.node();
    if kinds::CALL.contains(&node.kind())
        && let Some(func) = node.child_by_field_name("function")
        && let Some(field) = func
            .child_by_field_name("field")
            .or_else(|| func.child_by_field_name("property"))
        && kinds::node_bytes(&field, source) == b"unwrap"
    {
        *count += 1;
    }
    if !cursor.goto_first_child() {
        return;
    }
    loop {
        count_unwraps_recursive(cursor, source, count);
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    cursor.goto_parent();
}

register_analysis_rule!(UnwrapChain);
