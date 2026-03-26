//! Analysis rule: detect empty catch blocks.

use super::kinds;
use crate::TsNode;
use crate::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};

/// Analysis rule that detects empty catch blocks.
struct EmptyCatch;

/// [`AnalysisRule`] implementation for `EmptyCatch`.
impl AnalysisRule for EmptyCatch {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { "empty-catch" }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::CATCH }

    /// Checks the given node for empty catch block violations.
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

        Some(Hint::from_node(
            self,
            node,
            Severity::Warning,
            "Empty catch block swallows errors silently".into(),
            &["Don't swallow errors silently", "At minimum, log the error"],
        ))
    }
}

register_analysis_rule!(EmptyCatch);
