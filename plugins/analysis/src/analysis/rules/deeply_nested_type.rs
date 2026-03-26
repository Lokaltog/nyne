//! Analysis rule: detect deeply nested generic types.

use super::kinds;
use crate::TsNode;
use crate::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};

/// Maximum nesting depth for generic type parameters.
const MAX_TYPE_DEPTH: usize = 3;

/// Analysis rule that detects deeply nested generic types.
struct DeeplyNestedType;

/// [`AnalysisRule`] implementation for `DeeplyNestedType`.
impl AnalysisRule for DeeplyNestedType {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { "deeply-nested-type" }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::TYPE_ANNOTATION }

    /// Checks the given node for deeply nested generic type violations.
    fn check(&self, node: TsNode<'_>, _context: &AnalysisContext<'_>) -> Option<Hint> {
        let raw = node.raw();

        // Only trigger on the outermost type — skip if parent is also a type.
        // Also skip if already inside a type alias — suggesting "extract a type
        // alias" for the RHS of an existing alias is nonsensical.
        if let Some(parent) = raw.parent()
            && (kinds::TYPE_ANNOTATION.contains(&parent.kind())
                || kinds::GENERIC_TYPE.contains(&parent.kind())
                || kinds::GENERIC_TYPE_ARGS.contains(&parent.kind())
                || kinds::TYPE_ALIAS.contains(&parent.kind()))
        {
            return None;
        }

        let depth = max_generic_depth(raw);
        if depth < MAX_TYPE_DEPTH {
            return None;
        }

        Some(Hint::from_node_line(
            self,
            node,
            Severity::Info,
            format!(
                "Type `{}` has {depth} levels of nesting",
                kinds::node_str(&raw, node.source()).unwrap_or("(complex type)")
            ),
            &["Extract a type alias"],
        ))
    }
}

/// Compute the maximum nesting depth of generic type parameters.
///
/// Only `GENERIC_TYPE` nodes (the `<...>` levels) count as nesting.
/// `TYPE_ANNOTATION` nodes (plain type names) are traversed but don't
/// add depth — `Vec<String>` is depth 1, not 3.
fn max_generic_depth(node: tree_sitter::Node<'_>) -> usize {
    let mut max_child_depth = 0;
    let mut cursor = node.walk();

    for child in node.named_children(&mut cursor) {
        let kind = child.kind();
        if kinds::GENERIC_TYPE.contains(&kind)
            || kinds::GENERIC_TYPE_ARGS.contains(&kind)
            || kinds::TYPE_ANNOTATION.contains(&kind)
        {
            max_child_depth = max_child_depth.max(max_generic_depth(child));
        }
    }

    if kinds::GENERIC_TYPE.contains(&node.kind()) {
        1 + max_child_depth
    } else {
        max_child_depth
    }
}

register_analysis_rule!(DeeplyNestedType);
