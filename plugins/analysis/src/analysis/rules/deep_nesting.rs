//! Analysis rule: detect excessive code nesting.

use crate::TsNode;
use crate::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};

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

/// Analysis rule that detects excessive code nesting depth.
struct DeepNesting;

/// [`AnalysisRule`] implementation for `DeepNesting`.
impl AnalysisRule for DeepNesting {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { "deep-nesting" }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { NESTING_KINDS }

    /// Checks the given node for excessive code nesting violations.
    fn check(&self, node: TsNode<'_>, _context: &AnalysisContext<'_>) -> Option<Hint> {
        let depth = nesting_depth(node);
        if depth <= MAX_DEPTH {
            return None;
        }

        let start_line = node.raw().start_position().row;
        let end_line = node.raw().end_position().row;

        Some(Hint {
            rule_id: self.id(),
            severity: Severity::Warning,
            line_range: start_line..end_line,
            message: format!(
                "Nesting depth {depth} (threshold: {MAX_DEPTH}) — consider early returns, guard clauses, or extracting into a helper function"
            ),
            suggestions: vec![
                "Extract the inner block into a separate function".into(),
                "Use early returns / guard clauses to reduce nesting".into(),
            ],
        })
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
