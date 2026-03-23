//! Preview computation for staged edits before application.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use color_eyre::eyre::{Result, eyre};
use nyne::dispatch::activation::ActivationContext;
use nyne::dispatch::context::RequestContext;
use nyne::node::capabilities::Readable;
use nyne::types::vfs_path::VfsPath;

use super::StagingMap;
use super::staging::StagingKey;
use crate::edit::diff_action::DiffAction;
use crate::edit::plan::{EditOutcome, EditPlan, FileEditResult, ValidationResult};
use crate::syntax::decomposed::DecompositionCache;

/// `DiffAction` for a single symbol's staged edits.
///
/// Decomposes the source file at read time to avoid stale byte ranges.
#[derive(Clone)]
/// Preview of a symbol within a staged file.
pub(super) struct SymbolPreview {
    pub key: StagingKey,
    pub batches: StagingMap,
    pub ctx: Arc<ActivationContext>,
}

impl DiffAction for SymbolPreview {
    fn compute_edits(&self, _ctx: &RequestContext<'_>) -> Result<Vec<FileEditResult>> {
        let map = self.batches.read();
        let batch = map
            .get(&self.key)
            .ok_or_else(|| eyre!("no staged edits for {}", self.key.source_file))?;

        if batch.is_empty() {
            return Ok(Vec::new());
        }

        let parsed = self
            .ctx
            .get::<DecompositionCache>()
            .ok_or_else(|| eyre!("coding plugin not activated"))?
            .get(&self.key.source_file)?;

        let plan = batch.to_edit_plan();
        let resolved = plan.resolve(&parsed.decomposed, &parsed.source)?;
        let modified = EditPlan::apply(&parsed.source, &resolved);

        let validation = match parsed.decomposer.validate(&modified) {
            Ok(()) => ValidationResult::Pass,
            Err(e) => ValidationResult::Fail(format!("{}: {e}", self.key.source_file)),
        };

        Ok(vec![FileEditResult {
            source_file: self.key.source_file.clone(),
            display_path: self.key.source_file.as_str().to_owned(),
            original: parsed.source.clone(),
            modified,
            outcome: EditOutcome::Modify,
            validation,
        }])
    }

    fn header_lines(&self) -> Vec<String> {
        let count = self.batches.read().get(&self.key).map_or(0, |b| b.actions().count());
        let symbol = self.key.fragment_path.join("::");
        vec![format!(
            "Batch edit: {count} staged action(s) for {symbol} in {}",
            self.key.source_file
        )]
    }

    fn on_applied(&self, _ctx: &RequestContext<'_>) -> Result<()> {
        self.batches.write().remove(&self.key);
        Ok(())
    }
}

/// `DiffAction` for the cross-file root `@/edit/staged.diff`.
///
/// Combines previews for all symbols across all files with staged batches.
#[derive(Clone)]
/// Cross-file preview for multi-file edits.
pub(super) struct CrossFilePreview {
    pub batches: StagingMap,
    pub ctx: Arc<ActivationContext>,
}

impl DiffAction for CrossFilePreview {
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

        let mut results = Vec::new();
        for (source_file, plans) in &by_file {
            let Ok(parsed) = self
                .ctx
                .get::<DecompositionCache>()
                .ok_or_else(|| color_eyre::eyre::eyre!("coding plugin not activated"))?
                .get(source_file)
            else {
                continue;
            };

            // Merge all symbol batches for this file into one plan.
            let all_ops: Vec<_> = plans.iter().flat_map(|p| p.ops.iter().cloned()).collect();
            let plan = EditPlan { ops: all_ops };

            let resolved = plan.resolve(&parsed.decomposed, &parsed.source)?;
            let modified = EditPlan::apply(&parsed.source, &resolved);

            let validation = match parsed.decomposer.validate(&modified) {
                Ok(()) => ValidationResult::Pass,
                Err(e) => ValidationResult::Fail(format!("{source_file}: {e}")),
            };

            results.push(FileEditResult {
                source_file: source_file.clone(),
                display_path: source_file.as_str().to_owned(),
                original: parsed.source.clone(),
                modified,
                outcome: EditOutcome::Modify,
                validation,
            });
        }

        Ok(results)
    }

    fn header_lines(&self) -> Vec<String> {
        let mut action_count = 0;
        let mut seen_files = HashSet::new();
        let map = self.batches.read();
        for (key, batch) in map.iter() {
            if batch.is_empty() {
                continue;
            }
            action_count += batch.actions().count();
            seen_files.insert(key.source_file.clone());
        }
        let file_count = seen_files.len();
        vec![format!(
            "Batch edit: {action_count} action(s) across {file_count} file(s)"
        )]
    }

    fn on_applied(&self, _ctx: &RequestContext<'_>) -> Result<()> {
        self.batches.write().clear();
        Ok(())
    }
}

/// Readable for a single staged action — returns the staged content.
pub(super) struct StagedActionContent {
    pub batches: StagingMap,
    pub key: StagingKey,
    pub index: u32,
}

impl Readable for StagedActionContent {
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
