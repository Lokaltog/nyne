//! Analysis rule: detect long parameter lists.

use crate::TsNode;
use crate::analysis::{AnalysisRule, Hint, Severity, register_analysis_rule};

pub const ID: &str = "long-parameter-list";
/// Maximum parameter count before triggering a hint.
const MAX_PARAMS: usize = 5;

/// Node kinds representing parameter lists (cross-language).
///
/// Tree-sitter reuses kind strings across grammars, so this list is
/// already deduplicated (Rust and Python both use `"parameters"`).
const PARAM_LIST_KINDS: &[&str] = &[
    "parameters",        // Rust, Python
    "formal_parameters", // TypeScript, JavaScript
];

/// Analysis rule that detects long parameter lists.
struct LongParameterList;

/// [`AnalysisRule`] implementation for `LongParameterList`.
impl AnalysisRule for LongParameterList {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { ID }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { PARAM_LIST_KINDS }

    /// Checks the given node for long parameter list violations.
    fn check(&self, node: TsNode<'_>) -> Option<Hint> {
        // Count named children that are actual parameters (skip delimiters like commas, parens).
        let param_count = node
            .raw()
            .named_children(&mut node.raw().walk())
            .filter(|child| {
                let kind = child.kind();
                // Skip non-parameter nodes that appear as named children.
                kind != "comment" && kind != "line_comment" && kind != "block_comment"
            })
            .count();

        if param_count <= MAX_PARAMS {
            return None;
        }

        Some(Hint::from_node(
            self,
            node,
            Severity::Warning,
            format!(
                "{param_count} parameters (threshold: {MAX_PARAMS}) — consider grouping related parameters into a struct or options object"
            ),
            &[
                "Group related parameters into a config/options struct",
                "Use the builder pattern for complex construction",
            ],
        ))
    }
}

register_analysis_rule!(LongParameterList);
