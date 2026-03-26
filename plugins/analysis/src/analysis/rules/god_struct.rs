//! Analysis rule: detect god structs.

use super::kinds;
use crate::TsNode;
use crate::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};

/// Maximum fields before triggering.
const MAX_FIELDS: usize = 10;

/// Analysis rule that detects structs with too many fields.
struct GodStruct;

/// [`AnalysisRule`] implementation for `GodStruct`.
impl AnalysisRule for GodStruct {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { "god-struct" }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::STRUCT_DEF }

    /// Checks the given node for god struct violations.
    fn check(&self, node: TsNode<'_>, _context: &AnalysisContext<'_>) -> Option<Hint> {
        let raw = node.raw();
        let count = count_fields(raw);

        if count <= MAX_FIELDS {
            return None;
        }

        let name = node.field_text("name").unwrap_or("(anonymous)");

        Some(Hint::from_node(
            self,
            node,
            Severity::Warning,
            format!("`{name}` has {count} fields (threshold: {MAX_FIELDS})"),
            &[
                "Consider splitting into smaller structs",
                "Group related fields into sub-structs",
            ],
        ))
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
