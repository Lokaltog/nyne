//! Analysis rule: detect structs with too many methods.

use super::kinds;
use crate::TsNode;
use crate::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};

pub const ID: &str = "too-many-methods";
/// Maximum methods in an impl/class block before triggering.
const MAX_METHODS: usize = 15;

/// Analysis rule that detects impl blocks with too many methods.
struct TooManyMethods;

/// [`AnalysisRule`] implementation for `TooManyMethods`.
impl AnalysisRule for TooManyMethods {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { ID }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::IMPL_BLOCK }

    /// Checks the given node for too many methods violations.
    fn check(&self, node: TsNode<'_>, _context: &AnalysisContext<'_>) -> Option<Hint> {
        let raw = node.raw();

        // Rust: methods live inside `declaration_list` (body field).
        // JS/TS/Python: methods are direct children.
        let method_count = kinds::count_children_of_kind(&raw, "body", kinds::FUNCTION);

        if method_count <= MAX_METHODS {
            return None;
        }

        let name = impl_name(raw, node.source()).unwrap_or("(anonymous)");

        Some(Hint::from_node(
            self,
            node,
            Severity::Warning,
            format!("`{name}` impl has {method_count} methods (threshold: {MAX_METHODS})"),
            &["Consider splitting into trait impls or helper modules"],
        ))
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
