//! Analysis rule: detect god structs with too many fields.
//!
//! Triggers when a struct/class definition has more than `MAX_FIELDS` (12)
//! field declarations. God structs accumulate unrelated state and become
//! difficult to construct, test, and evolve.
//!
//! **Why it matters:** A struct with many fields typically violates the Single
//! Responsibility Principle. Splitting into focused sub-structs or using the
//! builder pattern improves maintainability.
//!
//! **Example trigger:** A struct with 13+ fields will trigger this rule.
//!
//! **Cross-language:** Counts `field_declaration`, `field_definition`,
//! `property_declaration`, etc. across Rust, TypeScript, Python, Go, and Java.

use super::kinds;
use crate::TsNode;
use crate::analysis::{Hint, Rule, Severity, register_analysis_rule};

/// Unique identifier for this rule, used in configuration and hint output.
pub const ID: &str = "god-struct";
/// Maximum fields before triggering.
const MAX_FIELDS: usize = 10;

/// Analysis rule that detects structs with too many fields.
struct GodStruct;

/// [`Rule`] implementation for `GodStruct`.
impl Rule for GodStruct {
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
