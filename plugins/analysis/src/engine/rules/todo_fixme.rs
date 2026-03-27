//! Analysis rule: detect TODO and FIXME comments.
//!
//! Triggers on comment nodes containing `TODO:` or `FIXME:` markers (requires
//! a colon after the keyword to avoid false positives on prose discussion).
//!
//! **Why it matters:** TODO/FIXME markers represent deferred work that should
//! be tracked. Surfacing them as hints keeps them visible in analysis output
//! and prevents them from being forgotten.
//!
//! **Example triggers:**
//! ```text
//! // TODO: handle the error case properly
//! // FIXME: this panics on empty input
//! ```
//!
//! **Caveat:** Prose mentions like "the todo list" without a colon are
//! intentionally excluded to avoid false positives.

use super::kinds;
use crate::TsNode;
use crate::engine::{Hint, Rule, Severity, register_analysis_rule};

/// Unique identifier for this rule, used in configuration and hint output.
pub const ID: &str = "todo-fixme";
/// Analysis rule that detects TODO and FIXME comments.
struct TodoFixme;

/// [`Rule`] implementation for `TodoFixme`.
impl Rule for TodoFixme {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { ID }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::COMMENT }

    /// Checks the given node for TODO/FIXME comment violations.
    fn check(&self, node: TsNode<'_>) -> Option<Hint> {
        const TAGS: &[&str] = &["FIXME", "SAFETY", "HACK", "XXX", "TODO"];

        let text = node.text();

        let (marker, detail) = TAGS
            .iter()
            .find_map(|&tag| extract_marker_text(text, tag).map(|d| (tag, d)))?;

        let suffix = if detail.is_empty() {
            String::new()
        } else {
            format!(": {detail}")
        };

        Some(Hint::from_node(
            self,
            node,
            Severity::Info,
            format!("{marker} comment found{suffix}"),
            &[],
        ))
    }
}

/// Given text immediately after a tag keyword (e.g. the `": fix this"` in
/// `TODO: fix this`), skip an optional `(annotation)` and require a colon.
///
/// Returns `None` if no colon follows the tag -- bare mentions are not actionable.
fn parse_tag_suffix(after_tag: &str) -> Option<&str> {
    let rest = if after_tag.starts_with('(') {
        after_tag.find(')').map_or(after_tag, |pos| &after_tag[pos + 1..])
    } else {
        after_tag
    };
    Some(rest.strip_prefix(':')?.trim())
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
