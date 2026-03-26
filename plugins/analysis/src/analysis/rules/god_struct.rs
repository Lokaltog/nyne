//! Analysis rule: detect god structs.

use super::kinds;
use crate::TsNode;
use crate::analysis::{AnalysisRule, Hint, Severity, register_analysis_rule};

pub const ID: &str = "god-struct";
/// Maximum fields before triggering.
const MAX_FIELDS: usize = 10;

/// Analysis rule that detects structs with too many fields.
struct GodStruct;

/// [`AnalysisRule`] implementation for `GodStruct`.
impl AnalysisRule for GodStruct {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { ID }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::STRUCT_DEF }

    /// Checks the given node for god struct violations.
    fn check(&self, node: TsNode<'_>) -> Option<Hint> {
        let count = kinds::count_children_of_kind(&node.raw(), "body", kinds::FIELD_DECLARATION);

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

register_analysis_rule!(GodStruct);
