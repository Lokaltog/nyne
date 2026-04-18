//! Batch edit staging — in-memory accumulator for multi-symbol edits.
//!
//! Agents write to `file.rs@/symbols/Foo@/edit/replace` (and friends) to
//! stage edits, then `rm @/edit/staged.diff` to apply atomically with
//! tree-sitter validation. The staging area is per-mount, keyed by source
//! file.

use std::collections::HashMap;
use std::mem;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use color_eyre::eyre::Result;
use nyne::router::{AffectedFiles, Writable, WriteContext};
use nyne_diff::{DiffSource, FileEditResult, ValidationResult};
use parking_lot::Mutex;

use crate::edit::plan::{EditOp, EditOpKind, EditPlan};
use crate::syntax::SyntaxRegistry;
use crate::syntax::decomposed::DecompositionCache;

/// Per-mount staging area for batch edits.
///
/// Thread-safe: multiple concurrent writes stage independently. The `u32`
/// sequence number preserves global insertion order for deterministic
/// conflict reporting and diff output. Cloning bumps a single `Arc`.
#[derive(Clone, Default)]
pub struct EditStaging {
    inner: Arc<StagingInner>,
}

/// Shared state behind `Arc<StagingInner>` — keeps the mutex and counter
/// in one allocation so [`EditStaging`] clones bump a single refcount.
#[derive(Default)]
struct StagingInner {
    ops: Mutex<HashMap<PathBuf, Vec<(u32, EditOp)>>>,
    counter: AtomicU32,
}

impl EditStaging {
    /// Create an empty staging area.
    pub fn new() -> Self { Self::default() }

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
        let seq = self.inner.counter.fetch_add(1, Ordering::Relaxed);
        self.inner
            .ops
            .lock()
            .entry(source_file)
            .or_default()
            .push((seq, EditOp {
                fragment_path,
                kind,
                content,
            }));
        seq
    }

    /// Take all staged operations, leaving the staging area empty.
    pub fn drain(&self) -> HashMap<PathBuf, Vec<(u32, EditOp)>> { mem::take(&mut *self.inner.ops.lock()) }

    /// Drain only the operations staged against `source_file`, leaving
    /// other files' operations intact. Returns the removed entries.
    ///
    /// Used by scoped [`BatchEditAction`] and [`ClearWritable`] for the
    /// per-symbol `<file>@/symbols/Foo@/edit/staged.diff` endpoint.
    pub fn drain_file(&self, source_file: &Path) -> Vec<(u32, EditOp)> {
        self.inner.ops.lock().remove(source_file).unwrap_or_default()
    }

    /// Discard all staged operations.
    pub fn clear(&self) {
        self.inner.ops.lock().clear();
        self.inner.counter.store(0, Ordering::Relaxed);
    }

    /// Whether there are any staged operations.
    pub fn is_empty(&self) -> bool { self.inner.ops.lock().is_empty() }

    /// Snapshot current staged operations for diff preview.
    pub fn snapshot(&self) -> HashMap<PathBuf, Vec<(u32, EditOp)>> { self.inner.ops.lock().clone() }

    /// Build a `staged.diff` node with [`ClearWritable`] attached, scoped
    /// either to a single source file or to the mount root.
    ///
    /// SSOT for "what the `staged.diff` node looks like" — consumed by
    /// both the mount-root extension registration (`scope = None`) and
    /// the per-file fragment route (`scope = Some(source_file)`).
    pub fn staged_diff_node(&self, scope: Option<PathBuf>, name: &str) -> nyne::router::NamedNode {
        nyne::router::Node::file()
            .with_writable(ClearWritable {
                staging: self.clone(),
                scope,
            })
            .named(name)
    }
}

/// [`DiffSource`] that computes edits from the current staging area.
///
/// The diff middleware renders a preview on read and applies on
/// `rm staged.diff` via [`DiffCapable`] request state.
///
/// When `scope` is `Some`, the action operates on a single source file
/// — used for the per-symbol `<file>@/symbols/Foo@/edit/staged.diff`
/// endpoint, which only surfaces edits staged against that specific
/// file. When `scope` is `None`, the action operates mount-wide — used
/// for the root `@/edit/staged.diff` endpoint, which aggregates edits
/// across every file.
///
/// [`DiffCapable`]: nyne_diff::DiffCapable
#[derive(Clone)]
pub struct BatchEditAction {
    pub(crate) staging: EditStaging,
    pub(crate) decomposition: DecompositionCache,
    pub(crate) registry: Arc<SyntaxRegistry>,
    /// Restrict the diff/apply/clear to a single source file. `None`
    /// means mount-wide (the root-level aggregation).
    pub(crate) scope: Option<PathBuf>,
}

impl BatchEditAction {
    /// Build a new action.
    ///
    /// `scope = None` aggregates mount-wide; `scope = Some(path)` restricts
    /// to a single source file. Single source of truth for the field list —
    /// both the mount-root registration and per-file fragment route go
    /// through here.
    pub fn new(
        staging: EditStaging,
        decomposition: DecompositionCache,
        registry: Arc<SyntaxRegistry>,
        scope: Option<PathBuf>,
    ) -> Self {
        Self {
            staging,
            decomposition,
            registry,
            scope,
        }
    }

    /// Snapshot staged operations visible to this action.
    ///
    /// Single source of truth for the scope filter: mount-wide actions
    /// see the full staging area, scoped actions see only ops whose
    /// `source_file` matches `self.scope`.
    fn scoped_snapshot(&self) -> HashMap<PathBuf, Vec<(u32, EditOp)>> {
        let mut snapshot = self.staging.snapshot();
        if let Some(scope) = &self.scope {
            snapshot.retain(|path, _| path == scope);
        }
        snapshot
    }

    /// Wire this action into `req`: set the diff capability (preview
    /// on read, apply on `rm`) and attach the `staged.diff` node with
    /// [`ClearWritable`] (`> staged.diff` drains the scoped area).
    ///
    /// Single source of truth for surfacing the diff capability — used
    /// by both the mount-root registration and the per-file fragment
    /// route.
    pub fn attach_to(&self, req: &mut nyne::router::Request, fs: &Arc<dyn nyne::router::Filesystem>, name: &str) {
        use nyne_diff::DiffRequest;
        req.set_diff_source(self.clone(), Arc::clone(fs));
        req.nodes.add(self.staging.staged_diff_node(self.scope.clone(), name));
    }
}

/// Build the per-file validation closure used by [`BatchEditAction::compute_edits`].
///
/// Returns [`ValidationResult::Skipped`] when the registry has no decomposer
/// for the file, [`Pass`](ValidationResult::Pass) on clean parse, and
/// [`Fail`](ValidationResult::Fail) carrying the parse error otherwise.
fn validator_for<'a>(
    registry: &'a SyntaxRegistry,
    source_file: &'a Path,
) -> impl FnOnce(&str) -> ValidationResult + 'a {
    move |modified| match registry.decomposer_for(source_file) {
        Some(decomposer) => match decomposer.validate(modified) {
            Ok(()) => ValidationResult::Pass,
            Err(e) => ValidationResult::Fail(format!("{e}")),
        },
        None => ValidationResult::Skipped,
    }
}

impl DiffSource for BatchEditAction {
    fn compute_edits(&self) -> Result<Vec<FileEditResult>> {
        let snapshot = self.scoped_snapshot();
        if snapshot.is_empty() {
            return Ok(Vec::new());
        }

        let mut results = Vec::with_capacity(snapshot.len());
        // Sort by path for deterministic output.
        let mut entries: Vec<_> = snapshot.into_iter().collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        for (source_file, ops) in entries {
            let parsed = self.decomposition.get(&source_file)?;
            let validator = validator_for(&self.registry, &source_file);
            results.push(EditPlan { ops }.run(&parsed, source_file.clone(), validator)?);
        }

        Ok(results)
    }

    fn header_lines(&self) -> Vec<String> {
        let snapshot = self.scoped_snapshot();
        let op_count: usize = snapshot.values().map(Vec::len).sum();
        match &self.scope {
            Some(scope) => vec![format!("Batch edit: {op_count} operation(s) in {}", scope.display())],
            None => vec![format!(
                "Batch edit: {op_count} operation(s) across {} file(s)",
                snapshot.len()
            )],
        }
    }

    fn on_applied(&self) -> Result<()> {
        match &self.scope {
            Some(scope) => {
                self.staging.drain_file(scope);
            }
            None => self.staging.clear(),
        }
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

/// [`Writable`] that clears staged edits on truncating write.
///
/// Attached to `staged.diff` so `> staged.diff` discards staged edits.
///
/// When `scope` is `Some(path)`, only operations staged against `path`
/// are drained — used for per-file `staged.diff` at
/// `<file>@/symbols/Foo@/edit/staged.diff`. When `scope` is `None`, the
/// full staging area is drained — used for the root `@/edit/staged.diff`.
#[derive(Clone)]
pub struct ClearWritable {
    pub staging: EditStaging,
    pub scope: Option<PathBuf>,
}

impl Writable for ClearWritable {
    fn write(&self, _ctx: &WriteContext<'_>, _data: &[u8]) -> Result<AffectedFiles> {
        // Drain instead of clear — return source file paths so cache
        // generations bump and staged.diff content is evicted.
        match &self.scope {
            Some(scope) => {
                self.staging.drain_file(scope);
                Ok(vec![scope.clone()])
            }
            None => Ok(self.staging.drain().into_keys().collect()),
        }
    }
}

#[cfg(test)]
mod tests;
