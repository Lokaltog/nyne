//! Analysis rule: detect functions with too many local variables.
//!
//! Triggers when a function body contains more than `MAX_LOCALS` (10)
//! `let`/`var`/`const` binding declarations. Functions with many locals
//! typically do too much and should be decomposed.
//!
//! **Why it matters:** High local variable count correlates with excessive
//! function complexity. Extract sub-functions, use structs to group related
//! state, or restructure the algorithm.
//!
//! **Example trigger:** A function with 11+ `let` bindings will trigger.
//!
//! **Note:** Counts bindings recursively into nested blocks (e.g. `if` bodies)
//! but not into nested closures or function definitions.

use super::kinds;
use crate::TsNode;
use crate::analysis::{Hint, Rule, Severity, register_analysis_rule};

/// Unique identifier for this rule, used in configuration and hint output.
pub const ID: &str = "too-many-locals";
/// Maximum local variable bindings before triggering.
const MAX_LOCALS: usize = 10;

/// Analysis rule that detects functions with too many local variables.
struct TooManyLocals;

/// [`Rule`] implementation for `TooManyLocals`.
impl Rule for TooManyLocals {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { ID }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::FUNCTION }

    /// Checks the given node for too many local variables violations.
    fn check(&self, node: TsNode<'_>) -> Option<Hint> {
        let body = node.raw().child_by_field_name("body")?;
        let count = count_bindings(body);

        if count <= MAX_LOCALS {
            return None;
        }

        Some(Hint::from_node(
            self,
            node,
            Severity::Warning,
            format!("{count} local bindings (threshold: {MAX_LOCALS}) — function may be doing too much"),
            &[
                "Extract related bindings into a helper function",
                "Group related state into a struct",
            ],
        ))
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
