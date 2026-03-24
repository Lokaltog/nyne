//! Analysis rule: detect empty catch blocks.

use super::kinds;
use crate::syntax::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};
use crate::syntax::parser::TsNode;

/// Analysis rule that detects empty catch blocks.
struct EmptyCatch;

/// [`AnalysisRule`] implementation for `EmptyCatch`.
impl AnalysisRule for EmptyCatch {
    fn id(&self) -> &'static str { "empty-catch" }

    fn node_kinds(&self) -> &'static [&'static str] { kinds::CATCH }

    fn check(&self, node: TsNode<'_>, _context: &AnalysisContext<'_>) -> Option<Hint> {
        let raw = node.raw();

        let body = raw.child_by_field_name("body").or_else(|| {
            let mut cursor = raw.walk();
            raw.named_children(&mut cursor)
                .find(|c| kinds::BLOCK.contains(&c.kind()))
        })?;

        let mut cursor = body.walk();
        let has_real_code = body
            .named_children(&mut cursor)
            .any(|c| !kinds::COMMENT.contains(&c.kind()));

        if has_real_code {
            return None;
        }

        let start_line = raw.start_position().row;
        let end_line = raw.end_position().row;

        Some(Hint {
            rule_id: self.id(),
            severity: Severity::Warning,
            line_range: start_line..end_line,
            message: "Empty catch block swallows errors silently".into(),
            suggestions: vec![
                "Don't swallow errors silently".into(),
                "At minimum, log the error".into(),
            ],
        })
    }
}

register_analysis_rule!(EmptyCatch);
