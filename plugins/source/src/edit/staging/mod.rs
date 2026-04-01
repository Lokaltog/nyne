//! Batch edit staging — in-memory accumulator for multi-symbol edits.
//!
//! Agents write to `file.rs@/symbols/Foo@/edit/replace` (and friends) to
//! stage edits, then `rm @/edit/staged.diff` to apply atomically with
//! tree-sitter validation. The staging area is per-mount, keyed by source
//! file.

use std::collections::HashMap;
use std::mem;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use color_eyre::eyre::Result;
use nyne::router::{AffectedFiles, Writable, WriteContext};
use nyne_diff::{DiffSource, EditOutcome, FileEditResult, ValidationResult};

use crate::edit::plan::{EditOp, EditOpKind, EditPlan};
use crate::syntax::SyntaxRegistry;
use crate::syntax::decomposed::DecompositionCache;

/// Staged operations for a single source file: `(sequence_number, op)` pairs.
pub type StagedOps = Vec<(u32, EditOp)>;

/// All staged operations keyed by absolute source file path.
pub type StagedOpsMap = HashMap<PathBuf, StagedOps>;

/// Per-mount staging area for batch edits.
///
/// Thread-safe: multiple concurrent writes stage independently. The `u32`
/// sequence number preserves global insertion order for deterministic
/// conflict reporting and diff output.
#[derive(Clone)]
pub struct EditStaging {
    ops: Arc<Mutex<StagedOpsMap>>,
    counter: Arc<AtomicU32>,
}

#[expect(clippy::expect_used, reason = "mutex poisoning is unrecoverable")]
impl EditStaging {
    /// Create an empty staging area.
    pub fn new() -> Self {
        Self {
            ops: Arc::new(Mutex::new(HashMap::new())),
            counter: Arc::new(AtomicU32::new(0)),
        }
    }

    /// Stage an edit operation for a source file.
    ///
    /// Returns the assigned sequence number.
    pub fn stage(
        &self,
        source_file: PathBuf,
        fragment_path: Vec<String>,
        kind: EditOpKind,
        content: Option<String>,
    ) -> u32 {
        let seq = self.counter.fetch_add(1, Ordering::Relaxed);
        self.ops
            .lock()
            .expect("staging lock poisoned")
            .entry(source_file)
            .or_default()
            .push((seq, EditOp::new(fragment_path, kind, content)));
        seq
    }

    /// Take all staged operations, leaving the staging area empty.
    pub fn drain(&self) -> StagedOpsMap { mem::take(&mut *self.ops.lock().expect("staging lock poisoned")) }

    /// Discard all staged operations.
    pub fn clear(&self) {
        self.ops.lock().expect("staging lock poisoned").clear();
        self.counter.store(0, Ordering::Relaxed);
    }

    /// Whether there are any staged operations.
    pub fn is_empty(&self) -> bool { self.ops.lock().expect("staging lock poisoned").is_empty() }

    /// Snapshot current staged operations for diff preview.
    pub fn snapshot(&self) -> StagedOpsMap { self.ops.lock().expect("staging lock poisoned").clone() }
}

impl Default for EditStaging {
    fn default() -> Self { Self::new() }
}

/// [`DiffSource`] that computes edits from the current staging area.
///
/// The diff middleware renders a preview on read and applies on
/// `rm staged.diff` via [`DiffCapable`] request state.
///
/// [`DiffCapable`]: nyne_diff::DiffCapable
#[derive(Clone)]
pub struct BatchEditAction {
    pub(crate) staging: EditStaging,
    pub(crate) decomposition: DecompositionCache,
    pub(crate) registry: Arc<SyntaxRegistry>,
}

impl DiffSource for BatchEditAction {
    fn compute_edits(&self) -> Result<Vec<FileEditResult>> {
        let snapshot = self.staging.snapshot();
        if snapshot.is_empty() {
            return Ok(Vec::new());
        }

        let mut results = Vec::with_capacity(snapshot.len());
        // Sort by path for deterministic output.
        let mut entries: Vec<_> = snapshot.into_iter().collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        for (source_file, ops) in entries {
            let parsed = self.decomposition.get(&source_file)?;
            let resolved = EditPlan { ops }.resolve(&parsed.decomposed, &parsed.source)?;
            let modified = EditPlan::apply(&parsed.source, &resolved);

            let validation = match self.registry.decomposer_for(&source_file) {
                Some(decomposer) => match decomposer.validate(&modified) {
                    Ok(()) => ValidationResult::Pass,
                    Err(e) => ValidationResult::Fail(format!("{e}")),
                },
                None => ValidationResult::Skipped,
            };

            results.push(FileEditResult {
                display_path: source_file.display().to_string(),
                source_file,
                original: parsed.source.clone(),
                modified,
                outcome: EditOutcome::Modify,
                validation,
            });
        }

        Ok(results)
    }

    fn header_lines(&self) -> Vec<String> {
        let snapshot = self.staging.snapshot();
        let op_count: usize = snapshot.values().map(Vec::len).sum();
        vec![format!(
            "Batch edit: {op_count} operation(s) across {} file(s)",
            snapshot.len()
        )]
    }

    fn on_applied(&self) -> Result<()> {
        self.staging.clear();
        Ok(())
    }
}

/// [`Writable`] that stages an edit operation on write.
///
/// Each `edit/{op}` file node in a fragment directory holds one of these.
#[derive(Clone)]
pub struct StageWritable {
    pub staging: EditStaging,
    pub source_file: PathBuf,
    pub fragment_path: Vec<String>,
    pub kind: EditOpKind,
}

impl Writable for StageWritable {
    fn write(&self, _ctx: &WriteContext<'_>, data: &[u8]) -> Result<AffectedFiles> {
        let content = match self.kind {
            EditOpKind::Delete => None,
            _ => Some(String::from_utf8_lossy(data).into_owned()),
        };
        self.staging
            .stage(self.source_file.clone(), self.fragment_path.clone(), self.kind, content);
        // Return the source file so the cache generation bumps and
        // staged.diff's CachedReadable is evicted on next read.
        Ok(vec![self.source_file.clone()])
    }
}

/// [`Writable`] that clears all staged edits on truncating write.
///
/// Attached to `staged.diff` so `> @/edit/staged.diff` discards all edits.
#[derive(Clone)]
pub struct ClearWritable {
    pub staging: EditStaging,
}

impl Writable for ClearWritable {
    fn write(&self, _ctx: &WriteContext<'_>, _data: &[u8]) -> Result<AffectedFiles> {
        // Drain instead of clear — return source file paths so cache
        // generations bump and staged.diff content is evicted.
        Ok(self.staging.drain().into_keys().collect())
    }
}

#[cfg(test)]
mod tests;
