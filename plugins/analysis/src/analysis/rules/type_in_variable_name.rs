//! Analysis rule: detect type names in variable names.

use super::kinds;
use crate::TsNode;
use crate::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};

/// Type-related suffixes/infixes that indicate encoding the type in the name.
const TYPE_FRAGMENTS: &[&str] = &[
    "_string", "_str", "_vec", "_map", "_hash", "_list", "_array", "_set", "_dict", "_tuple", "_bool", "_int",
    "_float", "_i32", "_i64", "_u32", "_u64", "_f32", "_f64", "_usize", "_isize", "string_", "str_", "vec_", "map_",
    "hash_", "list_", "array_", "set_", "dict_", "tuple_", "bool_", "int_", "float_", "i32_", "i64_", "u32_", "u64_",
    "f32_", "f64_", "usize_", "isize_",
];

/// Analysis rule that detects type names in variable names.
struct TypeInVariableName;

/// [`AnalysisRule`] implementation for `TypeInVariableName`.
impl AnalysisRule for TypeInVariableName {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { "type-in-variable-name" }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::BINDING }

    /// Checks the given node for type name in variable name violations.
    fn check(&self, node: TsNode<'_>, _context: &AnalysisContext<'_>) -> Option<Hint> {
        let raw = node.raw();
        let source = node.source();

        let name_node = raw
            .child_by_field_name("name")
            .or_else(|| raw.child_by_field_name("pattern"))?;
        if name_node.kind() != kinds::IDENTIFIER {
            return None;
        }

        let name = kinds::node_str(&name_node, source)?;

        // Only flag if the name actually contains a type fragment.
        let matched = TYPE_FRAGMENTS.iter().find(|frag| name.contains(**frag))?;

        Some(Hint::from_node_line(
            self,
            node,
            Severity::Info,
            format!(
                "Variable `{name}` encodes type `{}` in its name",
                matched.trim_matches('_'),
            ),
            &["Name for purpose, not type — the type is already visible"],
        ))
    }
}

register_analysis_rule!(TypeInVariableName);
