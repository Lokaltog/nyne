//! Analysis rule: detect long parameter lists.

use crate::syntax::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};
use crate::syntax::parser::TsNode;

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
    fn id(&self) -> &'static str { "long-parameter-list" }

    fn node_kinds(&self) -> &'static [&'static str] { PARAM_LIST_KINDS }

    fn check(&self, node: TsNode<'_>, _context: &AnalysisContext<'_>) -> Option<Hint> {
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

        let start_line = node.raw().start_position().row;
        let end_line = node.raw().end_position().row;

        Some(Hint {
            rule_id: self.id(),
            severity: Severity::Warning,
            line_range: start_line..end_line,
            message: format!(
                "{param_count} parameters (threshold: {MAX_PARAMS}) — consider grouping related parameters into a struct or options object"
            ),
            suggestions: vec![
                "Group related parameters into a config/options struct".into(),
                "Use the builder pattern for complex construction".into(),
            ],
        })
    }
}

register_analysis_rule!(LongParameterList);
