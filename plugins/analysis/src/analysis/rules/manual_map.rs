//! Analysis rule: suggest manual map patterns.

use super::kinds;
use crate::TsNode;
use crate::analysis::{AnalysisRule, Hint, Severity, register_analysis_rule};

pub const ID: &str = "manual-map";
/// Analysis rule that detects manual map patterns.
struct ManualMap;

/// [`AnalysisRule`] implementation for `ManualMap`.
impl AnalysisRule for ManualMap {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { ID }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::MATCH }

    /// Checks the given node for manual map pattern violations.
    fn check(&self, node: TsNode<'_>) -> Option<Hint> {
        let raw = node.raw();
        let source = node.source();

        // Rust: arms live inside `match_block` (body field).
        let body = raw.child_by_field_name("body").unwrap_or(raw);
        let mut cursor = body.walk();

        // Collect match arms/cases.
        let arms: Vec<_> = body
            .named_children(&mut cursor)
            .filter(|c| kinds::MATCH_ARM.contains(&c.kind()))
            .collect();

        // Exactly 2 arms for the Some/None pattern.
        let [first, second] = arms.as_slice() else {
            return None;
        };

        let (some_arm, none_arm) = if is_some_arm(first, source) && is_none_arm(second, source) {
            (first, second)
        } else if is_none_arm(first, source) && is_some_arm(second, source) {
            (second, first)
        } else {
            return None;
        };

        // The Some arm must produce Some(...) and the None arm must produce None.
        if !arm_body_wraps_some(some_arm, source) || !arm_body_is_none(none_arm, source) {
            return None;
        }

        Some(Hint::from_node(
            self,
            node,
            Severity::Info,
            "`match` on Option with Some → Some, None → None is a manual `.map()`".into(),
            &["Use `.map(|v| f(v))`"],
        ))
    }
}

/// Check whether a match arm pattern matches Some.
fn is_some_arm(arm: &tree_sitter::Node<'_>, source: &[u8]) -> bool {
    if let Some(pattern) = arm.child_by_field_name("pattern") {
        return kinds::node_bytes(&pattern, source).starts_with(b"Some(");
    }
    false
}

/// Check whether a match arm pattern matches None.
fn is_none_arm(arm: &tree_sitter::Node<'_>, source: &[u8]) -> bool {
    if let Some(pattern) = arm.child_by_field_name("pattern") {
        return kinds::node_bytes(&pattern, source) == b"None";
    }
    false
}

/// Check whether a match arm body wraps a value in Some.
fn arm_body_wraps_some(arm: &tree_sitter::Node<'_>, source: &[u8]) -> bool {
    if let Some(body) = arm
        .child_by_field_name("value")
        .or_else(|| arm.child_by_field_name("body"))
    {
        return kinds::node_bytes(&body, source).starts_with(b"Some(");
    }
    false
}

/// Check whether a match arm body evaluates to None or null.
fn arm_body_is_none(arm: &tree_sitter::Node<'_>, source: &[u8]) -> bool {
    if let Some(body) = arm
        .child_by_field_name("value")
        .or_else(|| arm.child_by_field_name("body"))
    {
        return kinds::node_bytes(&body, source) == b"None";
    }
    false
}

register_analysis_rule!(ManualMap);
