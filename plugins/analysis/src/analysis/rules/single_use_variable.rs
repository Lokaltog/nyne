//! Analysis rule: detect single-use variables that could be inlined.
//!
//! Triggers on `let` bindings where the variable is used exactly once in the
//! immediately following statement and nowhere else. The binding adds an
//! unnecessary indirection.
//!
//! **Why it matters:** Single-use variables add a name without aiding
//! readability. Inlining the expression reduces line count and makes data
//! flow more direct. Exception: when the variable name documents a complex
//! expression.
//!
//! **Example trigger:**
//! ```rust
//! let path = format!("{}/config.toml", dir);
//! std::fs::read_to_string(path)?;
//! // Prefer: std::fs::read_to_string(format!("{}/config.toml", dir))?;
//! ```
//!
//! **Caveat:** Disabled by default (`DEFAULT_DISABLED_RULES`) because
//! descriptive variable names often improve readability even when used once.

use super::kinds;
use crate::TsNode;
use crate::analysis::{Hint, Rule, Severity, register_analysis_rule};

/// Unique identifier for this rule, used in configuration and hint output.
pub const ID: &str = "single-use-variable";
/// Analysis rule that detects single-use variables.
struct SingleUseVariable;

/// [`Rule`] implementation for `SingleUseVariable`.
impl Rule for SingleUseVariable {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { ID }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::BINDING }

    /// Checks the given node for single-use variable violations.
    fn check(&self, node: TsNode<'_>) -> Option<Hint> {
        let raw = node.raw();
        let source = node.source();

        let name = binding_name(raw, source)?;

        // Must have a value (not just a declaration).
        raw.child_by_field_name("value")?;

        // Get next named sibling — the sole consumer.
        let next = raw.next_named_sibling()?;

        if !is_sole_use(name.as_bytes(), &next, source) {
            return None;
        }

        Some(Hint {
            rule_id: self.id(),
            severity: Severity::Info,
            line_range: raw.start_position().row..next.end_position().row,
            message: format!("`{name}` is bound and immediately consumed — consider inlining"),
            suggestions: &["Inline the expression — intermediate binding adds no clarity"],
        })
    }
}

/// Extract a simple identifier name from a binding node.
fn binding_name<'a>(node: tree_sitter::Node<'_>, source: &'a [u8]) -> Option<&'a str> {
    let name_node = node
        .child_by_field_name("name")
        .or_else(|| node.child_by_field_name("pattern"))?;
    if name_node.kind() != kinds::IDENTIFIER {
        return None;
    }
    kinds::node_str(&name_node, source)
}

/// Check that `name` appears exactly once in `next` and zero times in later siblings.
fn is_sole_use(name: &[u8], next: &tree_sitter::Node<'_>, source: &[u8]) -> bool {
    if kinds::count_identifier_uses(next, name, source) != 1 {
        return false;
    }
    let mut sibling = next.next_named_sibling();
    while let Some(s) = sibling {
        if kinds::count_identifier_uses(&s, name, source) > 0 {
            return false;
        }
        sibling = s.next_named_sibling();
    }
    true
}

register_analysis_rule!(SingleUseVariable);
