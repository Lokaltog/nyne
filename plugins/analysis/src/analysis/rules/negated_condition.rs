//! Analysis rule: detect negated conditions.

use super::kinds;
use crate::TsNode;
use crate::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};

/// Analysis rule that detects negated conditions with else branches.
struct NegatedCondition;

/// [`AnalysisRule`] implementation for `NegatedCondition`.
impl AnalysisRule for NegatedCondition {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { "negated-condition" }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::IF }

    /// Checks the given node for negated condition violations.
    fn check(&self, node: TsNode<'_>, _context: &AnalysisContext<'_>) -> Option<Hint> {
        let raw = node.raw();

        // Must have an else branch.
        raw.child_by_field_name("alternative")?;

        // Get the condition.
        let condition = raw.child_by_field_name("condition")?;

        // The condition must be a top-level unary `!` / `not`.
        let is_negated = if kinds::UNARY_NOT.contains(&condition.kind()) {
            is_not_operator(&condition, node.source())
        } else {
            // Some grammars wrap condition in parenthesized_expression.
            condition
                .named_child(0)
                .is_some_and(|inner| kinds::UNARY_NOT.contains(&inner.kind()) && is_not_operator(&inner, node.source()))
        };

        if !is_negated {
            return None;
        }

        Some(Hint::from_node(
            self,
            node,
            Severity::Info,
            "Negated condition with else branch — flip branches to remove negation".into(),
            &["Flip branches to remove negation"],
        ))
    }
}

/// Check that a unary expression is specifically `!` or `not` (not `-` or `~`).
fn is_not_operator(node: &tree_sitter::Node<'_>, source: &[u8]) -> bool {
    // Python: `not_operator` is already specific.
    if node.kind() == "not_operator" {
        return true;
    }
    // Rust/JS/TS: `unary_expression` — check the operator child.
    if let Some(op) = node.child_by_field_name("operator").or_else(|| node.child(0)) {
        return kinds::node_bytes(&op, source) == b"!";
    }
    false
}

register_analysis_rule!(NegatedCondition);
