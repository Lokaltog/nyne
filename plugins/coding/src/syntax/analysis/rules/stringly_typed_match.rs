//! Analysis rule: detect match expressions dispatching on string literals.
//!
//! Triggers when a `match` / `switch` has 3+ arms with string literal patterns,
//! suggesting a missing enum for type-safe dispatch.

use super::kinds;
use crate::syntax::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};
use crate::syntax::parser::TsNode;

/// Minimum string-literal arms before triggering.
const MIN_STRING_ARMS: usize = 3;

struct StringlyTypedMatch;

impl AnalysisRule for StringlyTypedMatch {
    fn id(&self) -> &'static str { "stringly-typed-match" }

    fn node_kinds(&self) -> &'static [&'static str] { kinds::MATCH }

    fn check(&self, node: TsNode<'_>, _context: &AnalysisContext<'_>) -> Option<Hint> {
        let raw = node.raw();

        let body = raw.child_by_field_name("body").unwrap_or(raw);
        let mut cursor = body.walk();

        let string_arm_count = body
            .named_children(&mut cursor)
            .filter(|c| kinds::MATCH_ARM.contains(&c.kind()))
            .filter(|arm| arm_has_string_pattern(arm))
            .count();

        if string_arm_count < MIN_STRING_ARMS {
            return None;
        }

        let start_line = raw.start_position().row;
        let end_line = raw.end_position().row;

        Some(Hint {
            rule_id: self.id(),
            severity: Severity::Warning,
            line_range: start_line..end_line,
            message: format!(
                "Match with {string_arm_count} string literal arms — consider an enum for type-safe dispatch"
            ),
            suggestions: vec![
                "Define an enum and parse the string at the boundary".into(),
                "Use `strum::EnumString` or `FromStr` for string-to-enum conversion".into(),
            ],
        })
    }
}

/// Check if a match arm's pattern contains a string literal.
fn arm_has_string_pattern(arm: &tree_sitter::Node<'_>) -> bool {
    // Rust: match_arm → pattern field. JS: switch_case → value field.
    let pattern = arm
        .child_by_field_name("pattern")
        .or_else(|| arm.child_by_field_name("value"));

    let Some(pat) = pattern else { return false };
    contains_string_literal(&pat)
}

/// Recursively check if a node or its descendants is a string literal.
fn contains_string_literal(node: &tree_sitter::Node<'_>) -> bool {
    if kinds::STRING.contains(&node.kind()) {
        return true;
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .any(|child| contains_string_literal(&child))
}

register_analysis_rule!(StringlyTypedMatch);
