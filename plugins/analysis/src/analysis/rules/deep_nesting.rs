//! Analysis rule: detect excessive code nesting depth.
//!
//! Triggers when an `if`/`for`/`while`/`match`/`loop` node is nested deeper
//! than `MAX_DEPTH` (4) levels. Each enclosing control-flow construct
//! increments the depth counter.
//!
//! **Why it matters:** Deeply nested code is hard to follow, test, and modify.
//! Extract inner branches into helper functions or use early returns to
//! flatten control flow.
//!
//! **Example trigger:**
//! ```rust
//! if a {
//!     if b {
//!         for x in xs {
//!             if c {
//!                 match d { .. } // depth 5 — triggers
//!             }
//!         }
//!     }
//! }
//! ```
//!
//! **Cross-language:** Works across Rust, Python, TypeScript, Go, and more —
//! `NESTING_KINDS` maps control-flow node kinds from multiple grammars.

use crate::TsNode;
use crate::analysis::{Hint, Rule, Severity, register_analysis_rule};

/// Maximum nesting depth before triggering a hint.
const MAX_DEPTH: usize = 3;

/// Node kinds that contribute to nesting depth (cross-language).
///
/// Tree-sitter kind strings are shared across languages that use the same
/// grammar conventions, so this list is already deduplicated.
const NESTING_KINDS: &[&str] = &[
    // Expressions (Rust, Nix)
    "if_expression",
    "match_expression",
    "for_expression",
    "while_expression",
    "loop_expression",
    "try_expression",
    // Statements (Python, TypeScript, JavaScript)
    "if_statement",
    "match_statement",
    "for_statement",
    "for_in_statement",
    "while_statement",
    "do_statement",
    "switch_statement",
    "try_statement",
];

/// Unique identifier for this rule, used in configuration and hint output.
pub const ID: &str = "deep-nesting";
/// Analysis rule that detects excessive code nesting depth.
struct DeepNesting;

/// [`Rule`] implementation for `DeepNesting`.
impl Rule for DeepNesting {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { ID }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { NESTING_KINDS }

    /// Checks the given node for excessive code nesting violations.
    fn check(&self, node: TsNode<'_>) -> Option<Hint> {
        let depth = nesting_depth(node);
        if depth <= MAX_DEPTH {
            return None;
        }

        Some(Hint::from_node(
            self,
            node,
            Severity::Warning,
            format!(
                "Nesting depth {depth} (threshold: {MAX_DEPTH}) — consider early returns, guard clauses, or extracting into a helper function"
            ),
            &[
                "Extract the inner block into a separate function",
                "Use early returns / guard clauses to reduce nesting",
            ],
        ))
    }
}

/// Count the nesting level of a node (1 = the node itself, +1 per nesting ancestor).
fn nesting_depth(node: TsNode<'_>) -> usize {
    let mut depth = 1; // count the node itself
    let mut current = node.raw();
    while let Some(parent) = current.parent() {
        if NESTING_KINDS.contains(&parent.kind()) {
            depth += 1;
        }
        current = parent;
    }
    depth
}

register_analysis_rule!(DeepNesting);
