//! Analysis rule: detect boolean parameters.

use super::kinds;
use crate::TsNode;
use crate::analysis::{AnalysisRule, Hint, Severity, register_analysis_rule};

pub const ID: &str = "boolean-parameter";
/// Analysis rule that detects boolean function parameters.
struct BooleanParameter;

/// [`AnalysisRule`] implementation for `BooleanParameter`.
impl AnalysisRule for BooleanParameter {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { ID }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::FUNCTION }

    /// Checks the given node for boolean parameter violations.
    fn check(&self, node: TsNode<'_>) -> Option<Hint> {
        let raw = node.raw();
        let source = node.source();

        let params = raw.child_by_field_name("parameters")?;
        let mut cursor = params.walk();
        let bool_params: Vec<_> = params
            .named_children(&mut cursor)
            .filter(|p| has_bool_type(p, source))
            .collect();

        if bool_params.is_empty() {
            return None;
        }

        Some(Hint::from_node_line(
            self,
            node,
            Severity::Info,
            format!(
                "{} boolean parameter{} in function signature",
                bool_params.len(),
                if bool_params.len() == 1 { "" } else { "s" },
            ),
            &["Use an enum or separate functions for clarity"],
        ))
    }
}

/// Check whether a parameter node has a boolean type annotation.
fn has_bool_type(param: &tree_sitter::Node<'_>, source: &[u8]) -> bool {
    // Check `type` field (Rust, TypeScript).
    if let Some(ty) = param.child_by_field_name("type") {
        let text = kinds::node_bytes(&ty, source);
        return kinds::BOOL_TYPES.iter().any(|b| text == b.as_bytes());
    }
    // Python type annotations: `annotation` field.
    if let Some(ann) = param.child_by_field_name("annotation") {
        let text = kinds::node_bytes(&ann, source);
        return kinds::BOOL_TYPES.iter().any(|b| text == b.as_bytes());
    }
    false
}

register_analysis_rule!(BooleanParameter);
