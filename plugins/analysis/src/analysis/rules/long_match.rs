//! Analysis rule: detect overly long match expressions.

use super::kinds;
use crate::TsNode;
use crate::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};

/// Maximum match arms/cases before triggering.
const MAX_ARMS: usize = 8;

/// Analysis rule that detects match expressions with too many arms.
struct LongMatch;

/// [`AnalysisRule`] implementation for `LongMatch`.
impl AnalysisRule for LongMatch {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { "long-match" }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::MATCH }

    /// Checks the given node for overly long match expression violations.
    fn check(&self, node: TsNode<'_>, _context: &AnalysisContext<'_>) -> Option<Hint> {
        let raw = node.raw();

        // Rust: arms live inside `match_block` (body field).
        // JS/TS: cases are direct children of switch_statement.
        let body = raw.child_by_field_name("body").unwrap_or(raw);
        let mut cursor = body.walk();

        let arm_count = body
            .named_children(&mut cursor)
            .filter(|c| kinds::MATCH_ARM.contains(&c.kind()))
            .count();

        if arm_count <= MAX_ARMS {
            return None;
        }

        let start_line = raw.start_position().row;
        let end_line = raw.end_position().row;

        Some(Hint {
            rule_id: self.id(),
            severity: Severity::Warning,
            line_range: start_line..end_line,
            message: format!("Match/switch with {arm_count} arms (threshold: {MAX_ARMS})"),
            suggestions: vec!["Consider a trait, lookup map, or enum dispatch".into()],
        })
    }
}

register_analysis_rule!(LongMatch);
