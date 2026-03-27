//! Analysis rule: detect match expressions dispatching on string literals.
//!
//! Triggers when a `match` / `switch` has 3+ arms with string literal patterns,
//! suggesting a missing enum for type-safe dispatch.

use super::kinds;
use crate::TsNode;
use crate::engine::{Hint, Rule, Severity, register_analysis_rule};

/// Unique identifier for this rule, used in configuration and hint output.
pub const ID: &str = "stringly-typed-match";
/// Minimum string-literal arms before triggering.
const MIN_STRING_ARMS: usize = 3;

/// Analysis rule that detects string-literal match dispatching.
struct StringlyTypedMatch;

/// [`Rule`] implementation for `StringlyTypedMatch`.
impl Rule for StringlyTypedMatch {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { ID }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::MATCH }

    /// Checks the given node for stringly-typed match violations.
    fn check(&self, node: TsNode<'_>) -> Option<Hint> {
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

        Some(Hint::from_node(
            self,
            node,
            Severity::Warning,
            format!("Match with {string_arm_count} string literal arms — consider an enum for type-safe dispatch"),
            &[
                "Define an enum and parse the string at the boundary",
                "Use `strum::EnumString` or `FromStr` for string-to-enum conversion",
            ],
        ))
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
