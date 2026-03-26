//! Analysis rule: detect array indexing inside loops.
//!
//! Triggers when a loop body contains array subscript expressions like
//! `items[i]` where `i` is the loop variable. Direct indexing risks
//! out-of-bounds panics and obscures iteration intent.
//!
//! **Why it matters:** Iterator-based patterns (`.iter()`, `.enumerate()`,
//! `for item in &items`) are bounds-safe and more idiomatic. Index-based
//! loops often signal C-style thinking that misses language ergonomics.
//!
//! **Example trigger:**
//! ```rust
//! for i in 0..items.len() {
//!     process(items[i]); // triggers — prefer `for item in &items`
//! }
//! ```
//!
//! **Caveat:** Disabled by default (`DEFAULT_DISABLED_RULES`) because index
//! access is sometimes intentional (parallel arrays, sliding windows).

use super::kinds;
use crate::TsNode;
use crate::analysis::{AnalysisRule, Hint, Severity, register_analysis_rule};

/// Unique identifier for this rule, used in configuration and hint output.
pub const ID: &str = "index-in-loop";
/// Analysis rule that detects array indexing inside loops.
struct IndexInLoop;

/// [`AnalysisRule`] implementation for `IndexInLoop`.
impl AnalysisRule for IndexInLoop {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { ID }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::LOOP }

    /// Checks the given node for array indexing in loop violations.
    fn check(&self, node: TsNode<'_>) -> Option<Hint> {
        let raw = node.raw();
        let source = node.source();

        // Only `for` loops with a range pattern (for i in 0..len).
        if !raw.kind().starts_with("for") {
            return None;
        }

        // Get the loop variable name.
        let pattern = raw
            .child_by_field_name("pattern")
            .or_else(|| raw.child_by_field_name("left"))?;
        if pattern.kind() != kinds::IDENTIFIER {
            return None;
        }
        let loop_var = kinds::node_bytes(&pattern, source);

        // Check that the iterable is a range expression (0..len or 0..collection.len()).
        let iterable = raw
            .child_by_field_name("value")
            .or_else(|| raw.child_by_field_name("right"))?;
        if iterable.kind() != "range_expression" {
            return None;
        }

        // Check body for `collection[loop_var]` indexing.
        let body = raw.child_by_field_name("body")?;
        if !has_index_access(&body, loop_var, source) {
            return None;
        }

        Some(Hint::from_node(
            self,
            node,
            Severity::Info,
            "Indexing with loop variable inside a `for i in 0..len` loop".into(),
            &["Use iterator: `for item in &collection`"],
        ))
    }
}

/// Check if the subtree contains `something[loop_var]`.
fn has_index_access(node: &tree_sitter::Node<'_>, loop_var: &[u8], source: &[u8]) -> bool {
    if kinds::INDEX_EXPRESSION.contains(&node.kind()) {
        // Rust `index_expression` has positional children: [collection, index].
        // JS/TS `subscript_expression` may use `index` field.
        let idx = node.child_by_field_name("index").or_else(|| node.named_child(1));
        if let Some(idx) = idx
            && idx.kind() == kinds::IDENTIFIER
            && kinds::node_bytes(&idx, source) == loop_var
        {
            return true;
        }
    }
    let mut cursor = node.walk();
    if !cursor.goto_first_child() {
        return false;
    }
    loop {
        if has_index_access(&cursor.node(), loop_var, source) {
            return true;
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    false
}

register_analysis_rule!(IndexInLoop);
