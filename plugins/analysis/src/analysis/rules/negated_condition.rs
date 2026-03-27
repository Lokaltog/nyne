//! Analysis rule: detect negated conditions with else branches.
//!
//! Triggers on `if !condition { A } else { B }` patterns, suggesting the
//! condition be flipped to `if condition { B } else { A }` for readability.
//!
//! **Why it matters:** Negated conditions force readers to mentally invert
//! the logic. Putting the positive case first follows the "happy path first"
//! principle and reduces cognitive load.
//!
//! **Example trigger:**
//! ```rust
//! if !user.is_active() {
//!     show_error();
//! } else {
//!     process(user);
//! }
//! // Prefer: if user.is_active() { process(user); } else { show_error(); }
//! ```
//!
//! **Caveat:** Only triggers when there is an else branch. Standalone
//! `if !cond { .. }` (guard clauses) are fine.

use super::kinds;
use crate::TsNode;
use crate::analysis::{Hint, Rule, Severity, register_analysis_rule};

/// Unique identifier for this rule, used in configuration and hint output.
pub const ID: &str = "negated-condition";
/// Analysis rule that detects negated conditions with else branches.
struct NegatedCondition;

/// [`Rule`] implementation for `NegatedCondition`.
impl Rule for NegatedCondition {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { ID }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::IF }

    /// Checks the given node for negated condition violations.
    fn check(&self, node: TsNode<'_>) -> Option<Hint> {
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
