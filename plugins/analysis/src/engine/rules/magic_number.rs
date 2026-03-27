//! Analysis rule: detect magic numbers outside constant contexts.
//!
//! Triggers on numeric literals that appear outside constant declarations,
//! enum variants, default trait implementations, and similar safe contexts.
//! Trivial values (0, 1, 2) are excluded.
//!
//! **Why it matters:** Magic numbers obscure intent. A named constant like
//! `MAX_RETRIES` is self-documenting, greppable, and changeable in one place.
//!
//! **Example trigger:**
//! ```rust
//! if retries > 3 {
//!     ..
//! } // What does 3 mean? Use MAX_RETRIES.
//! ```
//!
//! **Caveat:** Disabled by default (`DEFAULT_DISABLED_RULES`) because numeric
//! literals in array sizes, bit shifts, and math expressions are often
//! intentional and produce false positives.

use super::kinds;
use crate::TsNode;
use crate::engine::{Hint, Rule, Severity, register_analysis_rule};

/// Unique identifier for this rule, used in configuration and hint output.
pub const ID: &str = "magic-number";
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

/// [`Rule`] implementation for `MagicNumber`.
impl Rule for MagicNumber {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { ID }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { NUMBER_KINDS }

    /// Checks the given node for magic number violations.
    fn check(&self, node: TsNode<'_>) -> Option<Hint> {
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
