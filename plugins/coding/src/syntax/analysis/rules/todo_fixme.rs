//! Analysis rule: detect TODO and FIXME comments.

use super::kinds;
use crate::config::CodingConfig;
use crate::providers::todo::parse_tag_suffix;
use crate::syntax::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};
use crate::syntax::parser::TsNode;

struct TodoFixme;

impl AnalysisRule for TodoFixme {
    fn id(&self) -> &'static str { "todo-fixme" }

    fn node_kinds(&self) -> &'static [&'static str] { kinds::COMMENT }

    fn check(&self, node: TsNode<'_>, context: &AnalysisContext<'_>) -> Option<Hint> {
        let text = node.text();

        let (marker, detail) = context
            .activation
            .get::<CodingConfig>()?
            .todo
            .tags
            .iter()
            .find_map(|tag| extract_marker_text(text, tag).map(|d| (tag.as_str(), d)))?;

        let start_line = node.raw().start_position().row;
        let end_line = node.raw().end_position().row;

        let suffix = if detail.is_empty() {
            String::new()
        } else {
            format!(": {detail}")
        };

        Some(Hint {
            rule_id: self.id(),
            severity: Severity::Info,
            line_range: start_line..end_line,
            message: format!("{marker} comment found{suffix}"),
            suggestions: vec![],
        })
    }
}

/// Find a marker keyword followed by a colon and extract the trailing text.
///
/// Returns `None` if the marker is absent or not followed by a colon.
/// Delegates to [`crate::providers::todo::parse_tag_suffix`] for the
/// colon requirement (SSOT).
fn extract_marker_text(comment: &str, marker: &str) -> Option<String> {
    let upper = comment.to_uppercase();
    let marker_upper = marker.to_uppercase();
    let idx = upper.find(&marker_upper)?;
    let suffix = parse_tag_suffix(&comment[idx + marker.len()..])?;
    Some(suffix.to_owned())
}

register_analysis_rule!(TodoFixme);
