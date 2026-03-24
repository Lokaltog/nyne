//! Analysis rule: detect god structs.

use super::kinds;
use crate::syntax::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};
use crate::syntax::parser::TsNode;

/// Maximum fields before triggering.
const MAX_FIELDS: usize = 10;

/// Analysis rule that detects structs with too many fields.
struct GodStruct;

/// [`AnalysisRule`] implementation for `GodStruct`.
impl AnalysisRule for GodStruct {
    fn id(&self) -> &'static str { "god-struct" }

    fn node_kinds(&self) -> &'static [&'static str] { kinds::STRUCT_DEF }

    fn check(&self, node: TsNode<'_>, _context: &AnalysisContext<'_>) -> Option<Hint> {
        let raw = node.raw();
        let count = count_fields(raw);

        if count <= MAX_FIELDS {
            return None;
        }

        let name = node.field_text("name").unwrap_or("(anonymous)");
        let start_line = raw.start_position().row;
        let end_line = raw.end_position().row;

        Some(Hint {
            rule_id: self.id(),
            severity: Severity::Warning,
            line_range: start_line..end_line,
            message: format!("`{name}` has {count} fields (threshold: {MAX_FIELDS})"),
            suggestions: vec![
                "Consider splitting into smaller structs".into(),
                "Group related fields into sub-structs".into(),
            ],
        })
    }
}

/// Count the number of fields in a struct or class body.
fn count_fields(node: tree_sitter::Node<'_>) -> usize {
    let body = node.child_by_field_name("body").unwrap_or(node);

    let mut cursor = body.walk();
    body.named_children(&mut cursor)
        .filter(|c| kinds::FIELD_DECLARATION.contains(&c.kind()))
        .count()
}

register_analysis_rule!(GodStruct);
