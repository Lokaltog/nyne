//! Analysis rule: detect single-use variables.

use super::kinds;
use crate::syntax::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};
use crate::syntax::parser::TsNode;

/// Analysis rule that detects single-use variables.
struct SingleUseVariable;

/// [`AnalysisRule`] implementation for `SingleUseVariable`.
impl AnalysisRule for SingleUseVariable {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { "single-use-variable" }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::BINDING }

    /// Checks the given node for single-use variable violations.
    fn check(&self, node: TsNode<'_>, _context: &AnalysisContext<'_>) -> Option<Hint> {
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

        let start_line = raw.start_position().row;
        let end_line = next.end_position().row;

        Some(Hint {
            rule_id: self.id(),
            severity: Severity::Info,
            line_range: start_line..end_line,
            message: format!("`{name}` is bound and immediately consumed — consider inlining"),
            suggestions: vec!["Inline the expression — intermediate binding adds no clarity".into()],
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
