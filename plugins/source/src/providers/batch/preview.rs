//! Preview computation for staged edits before application.
//!
//! Implements the [`DiffAction`] trait for both per-symbol and cross-file
//! scopes. Reading `staged.diff` triggers preview computation: staged edit
//! operations are resolved against current source content to produce a
//! unified diff. Deleting the file applies the edits atomically.

use std::collections::{HashMap, HashSet};

use color_eyre::eyre::eyre;
use nyne::node::capabilities::Readable;
use nyne::prelude::*;

use super::StagingMap;
use super::staging::StagingKey;
use crate::edit::diff_action::DiffAction;
use crate::edit::plan::{EditOutcome, EditPlan, FileEditResult, ValidationResult};
use crate::services::SourceServices;
use crate::syntax::decomposed::DecomposedSource;

/// [`DiffAction`] for a single symbol's staged edits.
///
/// Decomposes the source file at read time to avoid stale byte ranges —
/// never captures a snapshot of the decomposition, since the source may
/// have been modified between staging and previewing.
#[derive(Clone)]
pub(super) struct SymbolPreview {
    pub key: StagingKey,
    pub batches: StagingMap,
    pub ctx: Arc<ActivationContext>,
}

/// Resolve an edit plan against a decomposed source and produce a preview.
fn resolve_and_preview(source_file: &VfsPath, plan: &EditPlan, parsed: &DecomposedSource) -> Result<FileEditResult> {
    let resolved = plan.resolve(&parsed.decomposed, &parsed.source)?;
    let modified = EditPlan::apply(&parsed.source, &resolved);

    let validation = match parsed.decomposer.validate(&modified) {
        Ok(()) => ValidationResult::Pass,
        Err(e) => ValidationResult::Fail(format!("{source_file}: {e}")),
    };

    Ok(FileEditResult {
        source_file: source_file.clone(),
        display_path: source_file.as_str().to_owned(),
        original: parsed.source.clone(),
        modified,
        outcome: EditOutcome::Modify,
        validation,
    })
}
/// [`DiffAction`] implementation for [`SymbolPreview`].
impl DiffAction for SymbolPreview {
    /// Resolve staged edits for a single symbol and produce a diff preview.
    fn compute_edits(&self, _ctx: &RequestContext<'_>) -> Result<Vec<FileEditResult>> {
        let map = self.batches.read();
        let batch = map
            .get(&self.key)
            .ok_or_else(|| eyre!("no staged edits for {}", self.key.source_file))?;

        if batch.is_empty() {
            return Ok(Vec::new());
        }

        let parsed = SourceServices::get(&self.ctx)
            .decomposition
            .get(&self.key.source_file)?;

        let plan = batch.to_edit_plan();
        Ok(vec![resolve_and_preview(&self.key.source_file, &plan, &parsed)?])
    }

    /// Return a summary header describing the staged actions.
    fn header_lines(&self) -> Vec<String> {
        let count = self.batches.read().get(&self.key).map_or(0, |b| b.actions().count());
        let symbol = self.key.fragment_path.join("::");
        vec![format!(
            "Batch edit: {count} staged action(s) for {symbol} in {}",
            self.key.source_file
        )]
    }

    /// Clear staged edits for this symbol after successful application.
    fn on_applied(&self, _ctx: &RequestContext<'_>) -> Result<()> {
        self.batches.write().remove(&self.key);
        Ok(())
    }
}

/// [`DiffAction`] for the cross-file root `@/edit/staged.diff`.
///
/// Combines previews for all symbols across all files with staged batches
/// into a single unified diff. Applying this action atomically writes all
/// modified files and clears the entire staging area.
#[derive(Clone)]
pub(super) struct CrossFilePreview {
    pub batches: StagingMap,
    pub ctx: Arc<ActivationContext>,
}

/// [`DiffAction`] implementation for [`CrossFilePreview`].
impl DiffAction for CrossFilePreview {
    /// Resolve staged edits across all files, merging per-file batches.
    fn compute_edits(&self, _ctx: &RequestContext<'_>) -> Result<Vec<FileEditResult>> {
        // Group batches by source file — multiple symbols in the same file
        // must be combined into a single EditPlan to avoid conflicting byte ranges.
        let mut by_file: HashMap<VfsPath, Vec<EditPlan>> = HashMap::new();
        {
            let map = self.batches.read();
            for (key, batch) in map.iter().filter(|(_, b)| !b.is_empty()) {
                by_file
                    .entry(key.source_file.clone())
                    .or_default()
                    .push(batch.to_edit_plan());
            }
        }

        let services = SourceServices::get(&self.ctx);
        let mut results = Vec::new();
        for (source_file, plans) in &by_file {
            let Ok(parsed) = services.decomposition.get(source_file) else {
                continue;
            };

            // Merge all symbol batches for this file into one plan.
            let all_ops: Vec<_> = plans.iter().flat_map(|p| p.ops.iter().cloned()).collect();
            let plan = EditPlan { ops: all_ops };

            results.push(resolve_and_preview(source_file, &plan, &parsed)?);
        }

        Ok(results)
    }

    /// Return a summary header with total action and file counts.
    fn header_lines(&self) -> Vec<String> {
        let map = self.batches.read();
        let mut actions = 0;
        let mut seen_files = HashSet::new();
        for (key, batch) in map.iter() {
            if batch.is_empty() {
                continue;
            }
            actions += batch.actions().count();
            seen_files.insert(&key.source_file);
        }
        let (action_count, file_count) = (actions, seen_files.len());
        drop(map);
        vec![format!(
            "Batch edit: {action_count} action(s) across {file_count} file(s)"
        )]
    }

    /// Clear all staged edits after successful application.
    fn on_applied(&self, _ctx: &RequestContext<'_>) -> Result<()> {
        self.batches.write().clear();
        Ok(())
    }
}

/// Readable for a single staged action — returns the staged content.
///
/// Backs the preview files under `edit/staged/` (e.g. `10-replace.diff`).
/// Reading returns the raw content that was written when the action was staged.
pub(super) struct StagedActionContent {
    pub batches: StagingMap,
    pub key: StagingKey,
    pub index: u32,
}

/// [`Readable`] implementation for [`StagedActionContent`].
impl Readable for StagedActionContent {
    /// Return the staged action content as bytes.
    fn read(&self, _ctx: &RequestContext<'_>) -> Result<Vec<u8>> {
        let map = self.batches.read();
        let batch = map
            .get(&self.key)
            .ok_or_else(|| eyre!("no staged edits for {}", self.key.source_file))?;
        let action = batch
            .get(self.index)
            .ok_or_else(|| eyre!("staged action {} not found", self.index))?;
        Ok(action.op.content().as_bytes().to_vec())
    }
}
