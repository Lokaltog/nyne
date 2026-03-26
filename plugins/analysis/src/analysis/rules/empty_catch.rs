//! Analysis rule: detect empty catch/rescue blocks.
//!
//! Triggers on `catch`, `except`, `rescue`, or `on` blocks that contain no
//! statements (only whitespace or comments). Empty exception handlers silently
//! swallow errors, making failures invisible.
//!
//! **Why it matters:** Swallowed exceptions hide bugs and make debugging
//! extremely difficult. At minimum, log the error or add a comment explaining
//! why ignoring it is intentional.
//!
//! **Example trigger:**
//! ```python
//! try:
//!     risky_operation()
//! except Exception:
//!     pass  # this is fine — but an empty block is not
//! ```
//!
//! **Cross-language:** Works for Rust (`catch_clause` is rare), Python
//! (`except_clause`), TypeScript/Java (`catch_clause`), Ruby (`rescue`).

use super::kinds;
use crate::TsNode;
use crate::analysis::{AnalysisRule, Hint, Severity, register_analysis_rule};

/// Unique identifier for this rule, used in configuration and hint output.
pub const ID: &str = "empty-catch";
/// Analysis rule that detects empty catch blocks.
struct EmptyCatch;

/// [`AnalysisRule`] implementation for `EmptyCatch`.
impl AnalysisRule for EmptyCatch {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { ID }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::CATCH }

    /// Checks the given node for empty catch block violations.
    fn check(&self, node: TsNode<'_>) -> Option<Hint> {
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
