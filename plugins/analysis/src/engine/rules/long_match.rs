//! Analysis rule: detect overly long match expressions.
//!
//! Triggers when a `match`/`switch` expression has more than `MAX_ARMS` (12)
//! arms or cases. Long match expressions are hard to review and often signal
//! a missing abstraction (dispatch table, visitor pattern, or enum method).
//!
//! **Why it matters:** Each arm is an independent code path that needs
//! testing. Extract common patterns, use trait dispatch, or split into
//! sub-matches to keep match expressions manageable.
//!
//! **Example trigger:** A `match` with 13+ arms will trigger this rule.

use super::kinds;
use crate::TsNode;
use crate::engine::{Hint, Rule, Severity, register_analysis_rule};

/// Unique identifier for this rule, used in configuration and hint output.
pub const ID: &str = "long-match";
/// Maximum match arms/cases before triggering.
const MAX_ARMS: usize = 8;

/// Analysis rule that detects match expressions with too many arms.
struct LongMatch;

/// [`Rule`] implementation for `LongMatch`.
impl Rule for LongMatch {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { ID }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::MATCH }

    /// Checks the given node for overly long match expression violations.
    fn check(&self, node: TsNode<'_>) -> Option<Hint> {
        // Rust: arms live inside `match_block` (body field).
        // JS/TS: cases are direct children of switch_statement.
        let arm_count = kinds::count_children_of_kind(&node.raw(), "body", kinds::MATCH_ARM);

        if arm_count <= MAX_ARMS {
            return None;
        }

        Some(Hint::from_node(
            self,
            node,
            Severity::Warning,
            format!("Match/switch with {arm_count} arms (threshold: {MAX_ARMS})"),
            &["Consider a trait, lookup map, or enum dispatch"],
        ))
    }
}

register_analysis_rule!(LongMatch);
