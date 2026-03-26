//! Analysis rule: detect closures with large bodies.
//!
//! Closures exceeding a line threshold should be extracted to named functions
//! for testability and readability.

use crate::TsNode;
use crate::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};

/// Maximum closure body lines before triggering.
const MAX_CLOSURE_LINES: usize = 15;

/// Tree-sitter node kinds for closure expressions.
const CLOSURE: &[&str] = &[
    "closure_expression", // Rust
    "arrow_function",     // JavaScript, TypeScript
    "lambda",             // Python
];

/// Analysis rule that detects closures with large bodies.
struct LargeClosure;

/// [`AnalysisRule`] implementation for `LargeClosure`.
impl AnalysisRule for LargeClosure {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { "large-closure" }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { CLOSURE }

    /// Checks the given node for large closure violations.
    fn check(&self, node: TsNode<'_>, _context: &AnalysisContext<'_>) -> Option<Hint> {
        let raw = node.raw();
        let start_line = raw.start_position().row;
        let end_line = raw.end_position().row;
        let line_count = end_line.saturating_sub(start_line) + 1;

        if line_count <= MAX_CLOSURE_LINES {
            return None;
        }

        Some(Hint::from_node(
            self,
            node,
            Severity::Info,
            format!("Closure spans {line_count} lines (threshold: {MAX_CLOSURE_LINES})"),
            &["Extract to a named function for testability and readability"],
        ))
    }
}

register_analysis_rule!(LargeClosure);
