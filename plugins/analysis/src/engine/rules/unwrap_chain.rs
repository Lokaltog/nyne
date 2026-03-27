//! Analysis rule: detect chained `.unwrap()` calls on method results.
//!
//! Triggers when a single expression or statement contains `MIN_UNWRAPS` (2)
//! or more `.unwrap()` calls. Chained unwraps create multiple potential panic
//! points with no context about which one failed.
//!
//! **Why it matters:** Each `.unwrap()` is a potential panic site. Chaining
//! them makes stack traces ambiguous — use `.expect("context")` or proper
//! error handling with `?` to provide meaningful failure messages.
//!
//! **Example trigger:**
//! ```rust
//! let value = map.get("key").unwrap().parse::<i32>().unwrap();
//! // Prefer: let value = map.get("key")?.parse::<i32>()?;
//! ```
//!
//! **Caveat:** Disabled by default (`DEFAULT_DISABLED_RULES`) because
//! `.unwrap()` is idiomatic in tests and prototyping code.

use super::kinds;
use crate::TsNode;
use crate::engine::{Hint, Rule, Severity, register_analysis_rule};

/// Unique identifier for this rule, used in configuration and hint output.
pub const ID: &str = "unwrap-chain";
/// Minimum `.unwrap()` calls to trigger (in a single statement or consecutive).
const MIN_UNWRAPS: usize = 2;

/// Analysis rule that detects chained unwrap calls.
struct UnwrapChain;

/// [`Rule`] implementation for `UnwrapChain`.
impl Rule for UnwrapChain {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { ID }

    /// Trigger on expression statements — we scan each for chained unwraps.
    fn node_kinds(&self) -> &'static [&'static str] { &[kinds::EXPRESSION_STATEMENT, "let_declaration"] }

    /// Checks the given node for unwrap chain violations.
    fn check(&self, node: TsNode<'_>) -> Option<Hint> {
        let raw = node.raw();
        let source = node.source();
        let count = count_unwrap_calls(raw, source);

        if count < MIN_UNWRAPS {
            return None;
        }

        Some(Hint::from_node_line(
            self,
            node,
            Severity::Warning,
            format!("{count} `.unwrap()` calls in one statement"),
            &["Propagate with `?` or use `let...else`"],
        ))
    }
}

/// Count the number of .`unwrap()` calls within a node subtree.
fn count_unwrap_calls(node: tree_sitter::Node<'_>, source: &[u8]) -> usize {
    let mut count = 0;
    let mut cursor = node.walk();
    count_unwraps_recursive(&mut cursor, source, &mut count);
    count
}

/// Recursively walk a subtree counting .`unwrap()` call nodes.
fn count_unwraps_recursive(cursor: &mut tree_sitter::TreeCursor<'_>, source: &[u8], count: &mut usize) {
    let node = cursor.node();
    if kinds::CALL.contains(&node.kind())
        && let Some(func) = node.child_by_field_name("function")
        && let Some(field) = func
            .child_by_field_name("field")
            .or_else(|| func.child_by_field_name("property"))
        && kinds::node_bytes(&field, source) == b"unwrap"
    {
        *count += 1;
    }
    if !cursor.goto_first_child() {
        return;
    }
    loop {
        count_unwraps_recursive(cursor, source, count);
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    cursor.goto_parent();
}

register_analysis_rule!(UnwrapChain);
