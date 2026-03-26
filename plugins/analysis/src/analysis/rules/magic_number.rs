//! Analysis rule: detect magic numbers.

use super::kinds;
use crate::TsNode;
use crate::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};

/// Node kinds for numeric literals (cross-language).
const NUMBER_KINDS: &[&str] = &[
    "integer_literal", // Rust
    "float_literal",   // Rust
    "number",          // JavaScript, TypeScript, Python
    "integer",         // Python
    "float",           // Python
];

/// Numbers that are almost never "magic" — too common to flag.
const TRIVIAL_VALUES: &[&str] = &["0", "1", "2", "0.0", "1.0"];

/// Additional safe parents specific to numeric literals.
const NUMERIC_SAFE_PARENTS: &[&str] = &[
    "index_expression",
    "range_expression",
    "range",
    "type_arguments",
    "type_identifier",
    "unary_expression",
    "prefix_expression",
];

/// Analysis rule that detects magic numbers.
struct MagicNumber;

/// [`AnalysisRule`] implementation for `MagicNumber`.
impl AnalysisRule for MagicNumber {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { "magic-number" }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { NUMBER_KINDS }

    /// Checks the given node for magic number violations.
    fn check(&self, node: TsNode<'_>, _context: &AnalysisContext<'_>) -> Option<Hint> {
        let text = node.text();

        if TRIVIAL_VALUES.contains(&text) {
            return None;
        }

        // Walk up to find the context — skip unary negation.
        let mut ancestor = node.raw().parent()?;
        if ancestor.kind() == "unary_expression" || ancestor.kind() == "prefix_expression" {
            ancestor = ancestor.parent()?;
        }

        if kinds::is_safe_literal_context(ancestor, NUMERIC_SAFE_PARENTS) {
            return None;
        }

        Some(Hint::from_node_line(
            self,
            node,
            Severity::Info,
            format!("Magic number `{text}` — extract to a named constant for clarity"),
            &["Extract to a `const` or `static` with a descriptive name"],
        ))
    }
}

register_analysis_rule!(MagicNumber);
