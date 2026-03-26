//! Analysis rule: detect unnecessary else blocks.

use super::kinds;
use crate::TsNode;
use crate::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};

/// Analysis rule that detects unnecessary else blocks.
struct UnnecessaryElse;

/// [`AnalysisRule`] implementation for `UnnecessaryElse`.
impl AnalysisRule for UnnecessaryElse {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { "unnecessary-else" }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::IF }

    /// Checks the given node for unnecessary else block violations.
    fn check(&self, node: TsNode<'_>, _context: &AnalysisContext<'_>) -> Option<Hint> {
        // Must have an else clause/alternative.
        let raw = node.raw();
        let else_node = raw
            .child_by_field_name("alternative")
            .filter(|n| n.kind() != "else_if" && n.kind() != "elif_clause")?;

        // The consequence (if-body) must end with an early exit.
        let consequence = raw.child_by_field_name("consequence")?;
        if !block_ends_with_exit(consequence) {
            return None;
        }

        // Don't flag if the else itself is an else-if chain — that's a different rule.
        if else_node.named_child_count() == 1 {
            let inner = else_node.named_child(0)?;
            if kinds::IF.contains(&inner.kind()) {
                return None;
            }
        }

        Some(Hint {
            rule_id: self.id(),
            severity: Severity::Warning,
            line_range: else_node.start_position().row..else_node.end_position().row,
            message: "Unnecessary `else` — the `if` branch already exits (return/continue/break)".into(),
            suggestions: &[
                "Remove the `else` and dedent the code",
                "Use early return / guard clause pattern",
            ],
        })
    }
}

/// Check if a block's last named child is an early exit statement.
///
/// Tree-sitter grammars may wrap returns in `expression_statement` (Rust)
/// or present them directly (Python), so we check both the node and its
/// first named child.
fn block_ends_with_exit(block: tree_sitter::Node<'_>) -> bool {
    let mut cursor = block.walk();
    let Some(last) = block.named_children(&mut cursor).last() else {
        return false;
    };
    if kinds::EXIT.contains(&last.kind()) {
        return true;
    }
    // Unwrap expression_statement / expression wrappers.
    last.named_child(0)
        .is_some_and(|inner| kinds::EXIT.contains(&inner.kind()))
}

register_analysis_rule!(UnnecessaryElse);
