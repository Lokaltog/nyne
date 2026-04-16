//! Symbol deletion with surrounding whitespace cleanup.
//!
//! Implements [`DiffSource`] so deletions can be previewed as a diff
//! (`Foo@/delete.diff`) or applied via the diff middleware.

use std::path::PathBuf;

use color_eyre::eyre::Result;
use nyne_diff::{DiffSource, FileEditResult, ValidationResult};

use crate::edit::plan::{EditOp, EditOpKind, EditPlan};
use crate::syntax::decomposed::DecompositionCache;

/// Delete a symbol from its source file.
///
/// Delegates to [`EditPlan`] for both resolution and application — the
/// delete logic (range extension, splice) is SSOT in `EditPlan::resolve()`.
///
/// Implements [`DiffSource`] — the diff middleware renders a preview on
/// read and applies on `rm delete.diff` via [`DiffCapable`] request state.
#[derive(Clone)]
pub(in crate::provider::syntax) struct SymbolDelete {
    pub decomposition: DecompositionCache,
    pub source_file: PathBuf,
    pub fragment_path: Vec<String>,
}

impl DiffSource for SymbolDelete {
    /// Compute the deletion range and produce file edits.
    fn compute_edits(&self) -> Result<Vec<FileEditResult>> {
        let parsed = self.decomposition.get(&self.source_file)?;
        let plan = EditPlan {
            ops: vec![(0, EditOp {
                fragment_path: self.fragment_path.clone(),
                kind: EditOpKind::Delete,
                content: None,
            })],
        };
        Ok(vec![
            plan.run(&parsed, self.source_file.clone(), |_| ValidationResult::Pass)?,
        ])
    }

    /// Return a header describing the deletion.
    fn header_lines(&self) -> Vec<String> { vec![format!("Delete symbol from {}", self.source_file.display())] }

    /// Invalidate caches after deletion.
    fn on_applied(&self) -> Result<()> {
        self.decomposition.invalidate(&self.source_file);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
