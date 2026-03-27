//! Symbol deletion with surrounding whitespace cleanup.
//!
//! Implements [`DiffAction`] so deletions can be previewed as a diff
//! (`Foo@/delete.diff`) or applied by unlinking the diff node.

use nyne::prelude::*;

use crate::edit::diff_action::DiffAction;
use crate::edit::plan::{EditOp, EditOpKind, EditOutcome, EditPlan, FileEditResult, ValidationResult};
use crate::services::Services;
use crate::syntax::decomposed::DecomposedSource;

/// Delete a symbol from its source file.
///
/// Delegates to [`EditPlan`] for both resolution and application — the
/// delete logic (range extension, splice) is SSOT in `EditPlan::resolve()`.
///
/// Implements [`DiffAction`] — wrap in [`DiffActionNode`] for `Readable`
/// (diff preview) and `Unlinkable` (apply-on-delete) capabilities.
///
/// [`DiffActionNode`]: crate::edit::diff_action::DiffActionNode
#[derive(Clone)]
pub(in crate::providers::syntax) struct SymbolDelete {
    pub ctx: Arc<ActivationContext>,
    pub source_file: VfsPath,
    pub fragment_path: Vec<String>,
}

/// Methods for [`SymbolDelete`].
impl SymbolDelete {
    /// Decompose the source file and return the shared decomposition.
    fn decomposed(&self) -> Result<Arc<DecomposedSource>> {
        Services::get(&self.ctx).decomposition.get(&self.source_file)
    }
}

/// [`DiffAction`] implementation for [`SymbolDelete`].
impl DiffAction for SymbolDelete {
    /// Compute the deletion range and produce file edits.
    fn compute_edits(&self, _ctx: &RequestContext<'_>) -> Result<Vec<FileEditResult>> {
        let parsed = self.decomposed()?;

        let plan = EditPlan {
            ops: vec![(0, EditOp {
                fragment_path: self.fragment_path.clone(),
                kind: EditOpKind::Delete,
                content: None,
            })],
        };

        let resolved = plan.resolve(&parsed.decomposed, &parsed.source)?;
        let modified = EditPlan::apply(&parsed.source, &resolved);

        Ok(vec![FileEditResult {
            source_file: self.source_file.clone(),
            display_path: self.source_file.as_str().to_owned(),
            original: parsed.source.clone(),
            modified,
            outcome: EditOutcome::Modify,
            validation: ValidationResult::Pass,
        }])
    }

    /// Return a header describing the deletion.
    fn header_lines(&self) -> Vec<String> { vec![format!("Delete symbol from {}", self.source_file)] }

    /// Invalidate caches and notify LSP after deletion.
    fn on_applied(&self, _ctx: &RequestContext<'_>) -> Result<()> {
        // Invalidate the decomposition cache so subsequent reads
        // (OVERVIEW, body, docstring, etc.) re-decompose from disk.
        Services::get(&self.ctx).decomposition.invalidate(&self.source_file);
        Ok(())
    }
}

/// Unit tests.
#[cfg(test)]
mod tests;
