//! Analysis rule: detect functions with too many local variables.

use super::kinds;
use crate::syntax::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};
use crate::syntax::parser::TsNode;

/// Maximum local variable bindings before triggering.
const MAX_LOCALS: usize = 10;

/// Analysis rule that detects functions with too many local variables.
struct TooManyLocals;

/// [`AnalysisRule`] implementation for `TooManyLocals`.
impl AnalysisRule for TooManyLocals {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { "too-many-locals" }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::FUNCTION }

    /// Checks the given node for too many local variables violations.
    fn check(&self, node: TsNode<'_>, _context: &AnalysisContext<'_>) -> Option<Hint> {
        let body = node.raw().child_by_field_name("body")?;
        let count = count_bindings(body);

        if count <= MAX_LOCALS {
            return None;
        }

        let start_line = node.raw().start_position().row;
        let end_line = node.raw().end_position().row;

        Some(Hint {
            rule_id: self.id(),
            severity: Severity::Warning,
            line_range: start_line..end_line,
            message: format!("{count} local bindings (threshold: {MAX_LOCALS}) — function may be doing too much"),
            suggestions: vec![
                "Extract related bindings into a helper function".into(),
                "Group related state into a struct".into(),
            ],
        })
    }
}

/// Count binding declarations in a block, recursing into nested blocks
/// but stopping at nested function boundaries.
fn count_bindings(node: tree_sitter::Node<'_>) -> usize {
    let mut count = 0;
    let mut cursor = node.walk();

    for child in node.named_children(&mut cursor) {
        let kind = child.kind();
        if kinds::BINDING.contains(&kind) {
            count += 1;
        } else if !kinds::FUNCTION.contains(&kind) {
            // Recurse into control flow blocks, but not nested functions.
            count += count_bindings(child);
        }
    }
    count
}

register_analysis_rule!(TooManyLocals);
