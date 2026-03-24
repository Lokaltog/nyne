//! Analysis rule: detect structs with too many methods.

use super::kinds;
use crate::syntax::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};
use crate::syntax::parser::TsNode;

/// Maximum methods in an impl/class block before triggering.
const MAX_METHODS: usize = 15;

/// Analysis rule that detects impl blocks with too many methods.
struct TooManyMethods;

/// [`AnalysisRule`] implementation for `TooManyMethods`.
impl AnalysisRule for TooManyMethods {
    fn id(&self) -> &'static str { "too-many-methods" }

    fn node_kinds(&self) -> &'static [&'static str] { kinds::IMPL_BLOCK }

    fn check(&self, node: TsNode<'_>, _context: &AnalysisContext<'_>) -> Option<Hint> {
        let raw = node.raw();

        // Rust: methods live inside `declaration_list` (body field).
        // JS/TS/Python: methods are direct children.
        let body = raw.child_by_field_name("body").unwrap_or(raw);
        let mut cursor = body.walk();

        let method_count = body
            .named_children(&mut cursor)
            .filter(|c| kinds::FUNCTION.contains(&c.kind()))
            .count();

        if method_count <= MAX_METHODS {
            return None;
        }

        let name = impl_name(raw, node.source()).unwrap_or("(anonymous)");
        let start_line = raw.start_position().row;
        let end_line = raw.end_position().row;

        Some(Hint {
            rule_id: self.id(),
            severity: Severity::Warning,
            line_range: start_line..end_line,
            message: format!("`{name}` impl has {method_count} methods (threshold: {MAX_METHODS})"),
            suggestions: vec!["Consider splitting into trait impls or helper modules".into()],
        })
    }
}

/// Extract the type name from an impl or class block node.
fn impl_name<'a>(node: tree_sitter::Node<'_>, source: &'a [u8]) -> Option<&'a str> {
    let type_node = node
        .child_by_field_name("type")
        .or_else(|| node.child_by_field_name("name"))?;
    kinds::node_str(&type_node, source)
}

register_analysis_rule!(TooManyMethods);
