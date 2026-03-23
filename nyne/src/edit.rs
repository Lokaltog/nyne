//! File edit result types and application.
//!
//! These are the output types of edit planning (which lives in `nyne-coding`).
//! They live in core because `node::diff_action` needs them for the
//! `DiffActionNode` unlink workflow.

use color_eyre::eyre::Result;

use crate::types::real_fs::RealFs;
use crate::types::vfs_path::VfsPath;

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
    Rename { new_path: VfsPath },
}

/// Result of tree-sitter validation on modified source.
#[derive(Clone)]
pub enum ValidationResult {
    /// Modified source parses without errors.
    Pass,
    /// Modified source has parse errors.
    Fail(String),
    /// Validation was skipped (e.g., no decomposer available, or LSP-sourced edits).
    Skipped,
}

/// A single file's edit result — original and modified content for diffing.
pub struct FileEditResult {
    /// Path for writing modified content back to disk (or file being deleted/renamed).
    pub source_file: VfsPath,
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

/// Apply a list of [`FileEditResult`]s to the filesystem.
///
/// Dispatches on [`EditOutcome`] for each result: modify, create, delete,
/// or rename. This is the single source of truth for applying file edits.
pub fn apply_file_edits(edits: &[FileEditResult], real_fs: &dyn RealFs) -> Result<()> {
    for edit in edits {
        match &edit.outcome {
            EditOutcome::Modify if edit.original == edit.modified => {}
            EditOutcome::Modify | EditOutcome::Create => {
                real_fs.write(&edit.source_file, edit.modified.as_bytes())?;
            }
            EditOutcome::Delete => real_fs.unlink(&edit.source_file)?,
            EditOutcome::Rename { new_path } => {
                real_fs.rename(&edit.source_file, new_path)?;
                if edit.original != edit.modified {
                    real_fs.write(new_path, edit.modified.as_bytes())?;
                }
            }
        }
    }
    Ok(())
}
