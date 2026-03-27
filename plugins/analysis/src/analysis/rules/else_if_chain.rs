//! Analysis rule: detect long else-if chains.
//!
//! Triggers when an `if` expression has `MIN_ELSE_IFS` (3) or more `else if`
//! branches, suggesting a `match`/`switch` expression or dispatch table would
//! be clearer.
//!
//! **Why it matters:** Long else-if chains are hard to scan, easy to mis-order,
//! and lack exhaustiveness checking. A `match` expression (or pattern matching
//! equivalent) makes the dispatch structure explicit.
//!
//! **Example trigger:**
//! ```rust
//! if kind == "a" {
//!     ..
//! } else if kind == "b" {
//!     ..
//! } else if kind == "c" {
//!     ..
//! } else if kind == "d" {
//!     ..
//! } // 3 else-ifs — triggers
//! ```

use super::kinds;
use crate::TsNode;
use crate::analysis::{Hint, Rule, Severity, register_analysis_rule};

/// Unique identifier for this rule, used in configuration and hint output.
pub const ID: &str = "else-if-chain";
/// Minimum else-if branches to trigger (3 = if + 3 else-ifs = 4 total).
const MIN_ELSE_IFS: usize = 3;

/// Analysis rule that detects long else-if chains.
struct ElseIfChain;

/// [`Rule`] implementation for `ElseIfChain`.
impl Rule for ElseIfChain {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { ID }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::IF }

    /// Checks the given node for else-if chain violations.
    fn check(&self, node: TsNode<'_>) -> Option<Hint> {
        // Only fire on the outermost if — don't re-fire on inner else-ifs.
        if let Some(parent) = node.raw().parent() {
            let pk = parent.kind();
            if pk == "else_clause" || pk == "elif_clause" || pk == "else" {
                return None;
            }
        }

        let count = count_else_ifs(node.raw());
        if count < MIN_ELSE_IFS {
            return None;
        }

        Some(Hint::from_node(
            self,
            node,
            Severity::Warning,
            format!("{count} chained else-if branches — consider a `match`/`switch` or lookup map"),
            &[
                "Refactor to a match/switch expression",
                "Use a HashMap/dict lookup for value mapping",
            ],
        ))
    }
}

/// Count else-if branches by walking the alternative chain.
fn count_else_ifs(mut node: tree_sitter::Node<'_>) -> usize {
    let mut count = 0;
    while let Some(alt) = node.child_by_field_name("alternative") {
        // The alternative might be an else_clause containing an if, or directly an if.
        let inner = if kinds::IF.contains(&alt.kind()) {
            alt
        } else {
            // Look for an if inside the else clause.
            match alt.named_child(0).filter(|c| kinds::IF.contains(&c.kind())) {
                Some(inner_if) => inner_if,
                None => break, // Plain else — end of chain.
            }
        };

        count += 1;
        node = inner;
    }
    count
}

register_analysis_rule!(ElseIfChain);
