//! Diff request state and the [`DiffSource`] trait.
//!
//! Producers set [`DiffCapable`] on the request to declare that a path
//! produces a diff. The middleware consumes it to build preview nodes
//! and apply edits.

use std::path::PathBuf;
use std::sync::Arc;

use color_eyre::eyre::{Result, WrapErr};
use nyne::router::{AffectedFiles, Filesystem, Request};

/// What file-level operation an edit performs.
#[derive(Debug)]
pub enum EditOutcome {
    /// Write modified content to an existing file.
    Modify,
    /// Create a new file with the modified content.
    Create,
    /// Delete the file entirely.
    Delete,
    /// Rename `source_file` to `new_path`, then write modified content.
    Rename { new_path: PathBuf },
}

/// Result of tree-sitter validation on modified source.
#[derive(Clone, Debug)]
pub enum ValidationResult {
    /// Modified source parses without errors.
    Pass,
    /// Modified source has parse errors.
    Fail(String),
    /// Validation was skipped (e.g., no decomposer available, or LSP-sourced edits).
    Skipped,
}

/// A single file's edit result — original and modified content for diffing.
#[derive(Debug)]
pub struct FileEditResult {
    /// Path for writing modified content back to disk (or file being deleted/renamed).
    pub source_file: PathBuf,
    /// Display path used in diff headers (e.g., `src/lib.rs`).
    pub display_path: String,
    /// Original file content (empty for `Create`).
    pub original: String,
    /// Modified file content (empty for `Delete`).
    pub modified: String,
    /// What file-level operation to perform on apply.
    pub outcome: EditOutcome,
    /// Tree-sitter validation result for the modified content.
    pub validation: ValidationResult,
}

impl FileEditResult {
    /// Create a result with [`ValidationResult::Skipped`] — used for LSP resource
    /// operations and resolved text edits where tree-sitter validation is not applicable.
    pub const fn skipped(
        source_file: PathBuf,
        display_path: String,
        original: String,
        modified: String,
        outcome: EditOutcome,
    ) -> Self {
        Self {
            source_file,
            display_path,
            original,
            modified,
            outcome,
            validation: ValidationResult::Skipped,
        }
    }
}

/// Apply a list of [`FileEditResult`]s to the filesystem.
///
/// Dispatches on [`EditOutcome`] for each result: modify, create, delete,
/// or rename. This is the single source of truth for applying file edits.
pub fn apply_file_edits(edits: &[FileEditResult], fs: &dyn Filesystem) -> Result<AffectedFiles> {
    let mut affected: AffectedFiles = Vec::new();
    for edit in edits {
        apply_single_edit(edit, fs, &mut affected)?;
    }
    Ok(affected)
}

fn apply_single_edit(edit: &FileEditResult, fs: &dyn Filesystem, affected: &mut AffectedFiles) -> Result<()> {
    match &edit.outcome {
        EditOutcome::Modify if edit.original == edit.modified => {}
        EditOutcome::Modify | EditOutcome::Create => {
            fs.write_file(&edit.source_file, edit.modified.as_bytes())
                .wrap_err_with(|| format!("failed to write {}", edit.source_file.display()))?;
            affected.push(edit.source_file.clone());
        }
        EditOutcome::Delete => {
            fs.remove(&edit.source_file)
                .wrap_err_with(|| format!("failed to delete {}", edit.source_file.display()))?;
            affected.push(edit.source_file.clone());
        }
        EditOutcome::Rename { new_path } => {
            fs.rename(&edit.source_file, new_path).wrap_err_with(|| {
                format!(
                    "failed to rename {} → {}",
                    edit.source_file.display(),
                    new_path.display()
                )
            })?;
            if edit.original != edit.modified {
                fs.write_file(new_path, edit.modified.as_bytes())
                    .wrap_err_with(|| format!("failed to write {}", new_path.display()))?;
            }
            affected.push(edit.source_file.clone());
            affected.push(new_path.clone());
        }
    }
    Ok(())
}

/// Trait for types that compute file edits for preview and application.
///
/// Implementors produce a list of per-file edit results. The diff middleware
/// uses this to render unified diff previews on read and apply edits on delete.
pub trait DiffSource: Send + Sync {
    /// Compute the edits this action would perform.
    ///
    /// Returns a list of per-file results with original and modified content.
    fn compute_edits(&self) -> Result<Vec<FileEditResult>>;

    /// Header lines describing this diff action.
    ///
    /// Each line is automatically prefixed with `# ` (diff comment syntax).
    /// An empty vec produces no header. A trailing `#\n` separator is appended
    /// after the last line.
    fn header_lines(&self) -> Vec<String> { Vec::new() }

    /// Post-apply hook — called after edits are successfully written to disk.
    ///
    /// Default is a no-op. Override to clear staging areas, invalidate caches, etc.
    fn on_applied(&self) -> Result<()> { Ok(()) }
}

/// Request state declaring that the current path produces a diff.
///
/// Set by content producers during lookup or remove. The diff middleware
/// consumes this to create preview nodes (lookup) or apply edits (remove).
#[derive(Clone)]
pub struct DiffCapable {
    /// The diff source that computes edits.
    pub source: Arc<dyn DiffSource>,
    /// Filesystem for applying edits to disk.
    pub fs: Arc<dyn Filesystem>,
}

/// Extension trait for registering a diff source on a request.
pub trait DiffRequest {
    /// Declare that the current path produces a diff.
    ///
    /// Sets [`DiffCapable`] state for the diff middleware to consume.
    fn set_diff_source(&mut self, source: impl DiffSource + 'static, fs: Arc<dyn Filesystem>);
}

impl DiffRequest for Request {
    fn set_diff_source(&mut self, source: impl DiffSource + 'static, fs: Arc<dyn Filesystem>) {
        self.set_state(DiffCapable {
            source: Arc::new(source),
            fs,
        });
    }
}
