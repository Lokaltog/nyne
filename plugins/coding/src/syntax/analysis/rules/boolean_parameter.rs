//! Analysis rule: detect boolean parameters.

use super::kinds;
use crate::syntax::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};
use crate::syntax::parser::TsNode;

struct BooleanParameter;

impl AnalysisRule for BooleanParameter {
    fn id(&self) -> &'static str { "boolean-parameter" }

    fn node_kinds(&self) -> &'static [&'static str] { kinds::FUNCTION }

    fn check(&self, node: TsNode<'_>, _context: &AnalysisContext<'_>) -> Option<Hint> {
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

        let start_line = raw.start_position().row;

        Some(Hint {
            rule_id: self.id(),
            severity: Severity::Info,
            line_range: start_line..start_line,
            message: format!(
                "{} boolean parameter{} in function signature",
                bool_params.len(),
                if bool_params.len() == 1 { "" } else { "s" },
            ),
            suggestions: vec!["Use an enum or separate functions for clarity".into()],
        })
    }
}

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
