// Symbol deletion — unified DiffAction for both preview and apply.

use std::sync::Arc;

use color_eyre::eyre::{Result, eyre};
use nyne::dispatch::activation::ActivationContext;
use nyne::dispatch::context::RequestContext;
use nyne::types::vfs_path::VfsPath;

use crate::edit::diff_action::DiffAction;
use crate::edit::plan::{EditOp, EditOutcome, EditPlan, FileEditResult, ValidationResult};
use crate::syntax::decomposed::DecompositionCache;
use crate::syntax::fragment::Fragment;

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
    /// Decompose the source file and return its fragments.
    fn fragments(&self) -> Result<(String, Vec<Fragment>)> {
        let parsed = self
            .ctx
            .get::<DecompositionCache>()
            .ok_or_else(|| eyre!("coding plugin not activated"))?
            .get(&self.source_file)?;
        Ok((parsed.source.clone(), parsed.decomposed.clone()))
    }
}

/// [`DiffAction`] implementation for [`SymbolDelete`].
impl DiffAction for SymbolDelete {
    /// Compute the deletion range and produce file edits.
    fn compute_edits(&self, _ctx: &RequestContext<'_>) -> Result<Vec<FileEditResult>> {
        let (source, fragments) = self.fragments()?;

        let plan = EditPlan {
            ops: vec![(0, EditOp::Delete {
                fragment_path: self.fragment_path.clone(),
            })],
        };

        let resolved = plan.resolve(&fragments, &source)?;
        let modified = EditPlan::apply(&source, &resolved);

        Ok(vec![FileEditResult {
            source_file: self.source_file.clone(),
            display_path: self.source_file.as_str().to_owned(),
            original: source,
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
        if let Some(cache) = self.ctx.get::<DecompositionCache>() {
            cache.invalidate(&self.source_file);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
