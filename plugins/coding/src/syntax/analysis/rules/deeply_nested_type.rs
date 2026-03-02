//! Analysis rule: detect deeply nested generic types.

use super::kinds;
use crate::syntax::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};
use crate::syntax::parser::TsNode;

/// Maximum nesting depth for generic type parameters.
const MAX_TYPE_DEPTH: usize = 3;

struct DeeplyNestedType;

impl AnalysisRule for DeeplyNestedType {
    fn id(&self) -> &'static str { "deeply-nested-type" }

    fn node_kinds(&self) -> &'static [&'static str] { kinds::TYPE_ANNOTATION }

    fn check(&self, node: TsNode<'_>, _context: &AnalysisContext<'_>) -> Option<Hint> {
        let raw = node.raw();

        // Only trigger on the outermost type — skip if parent is also a type.
        if let Some(parent) = raw.parent()
            && (kinds::TYPE_ANNOTATION.contains(&parent.kind())
                || kinds::GENERIC_TYPE.contains(&parent.kind())
                || kinds::GENERIC_TYPE_ARGS.contains(&parent.kind()))
        {
            return None;
        }

        let depth = max_generic_depth(raw);
        if depth < MAX_TYPE_DEPTH {
            return None;
        }

        let source = node.source();
        let type_text = kinds::node_str(&raw, source).unwrap_or("(complex type)");
        let line = raw.start_position().row;

        Some(Hint {
            rule_id: self.id(),
            severity: Severity::Info,
            line_range: line..line,
            message: format!("Type `{type_text}` has {depth} levels of nesting",),
            suggestions: vec!["Extract a type alias".into()],
        })
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
