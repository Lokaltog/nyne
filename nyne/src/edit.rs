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

/// Slice specification parsing for list-like virtual files.
///
/// Re-used by both `node::line_slice` (core) and `nyne-coding` providers.
pub mod slice {
    use std::ops::Range;

    /// A parsed range specification from a `:M`, `:M-N`, or `:-N` suffix.
    ///
    /// This is the generic slicing pattern described in the VFS spec — available
    /// on any list-like virtual file. Sliced paths are always lookup-only (hidden
    /// from readdir).
    ///
    /// All indices are 1-based and inclusive (matching `sed`/`awk` conventions).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum SliceSpec {
        /// Single item at 1-based index.
        Single(usize),
        /// Inclusive range `[start, end]`, both 1-based.
        Range(usize, usize),
        /// Last N items from the end (e.g., `:-10`).
        Tail(usize),
    }

    /// Parse a colon-suffixed slice spec from a name.
    ///
    /// Returns `Some((base_name, spec))` if the name ends with a valid `:…`
    /// suffix. Returns `None` if there is no colon, the base is empty, or the
    /// suffix doesn't parse.
    pub fn parse_slice_suffix(name: &str) -> Option<(&str, SliceSpec)> {
        let (base, spec_str) = name.rsplit_once(':')?;
        if base.is_empty() {
            return None;
        }
        let spec = parse_spec(spec_str)?;
        Some((base, spec))
    }

    /// Parse a bare spec string into a [`SliceSpec`].
    ///
    /// Accepts `"5"` (single), `"5-10"` (range), or `"-10"` (tail).
    /// Used by [`parse_slice_suffix`] and by route handlers that receive
    /// the spec as a captured route parameter.
    pub fn parse_spec(s: &str) -> Option<SliceSpec> {
        // Tail: "-N" where N > 0
        if let Some(rest) = s.strip_prefix('-') {
            if rest.is_empty() {
                return None;
            }
            let n: usize = rest.parse().ok()?;
            return (n > 0).then_some(SliceSpec::Tail(n));
        }

        // Range: "M-N"
        if let Some((start_str, end_str)) = s.split_once('-') {
            let start: usize = start_str.parse().ok()?;
            let end: usize = end_str.parse().ok()?;
            if start == 0 || end < start {
                return None;
            }
            return Some(SliceSpec::Range(start, end));
        }

        // Single: "M"
        let m: usize = s.parse().ok()?;
        (m > 0).then_some(SliceSpec::Single(m))
    }

    impl SliceSpec {
        /// Compute the 0-based half-open index range for a collection of `total` items.
        ///
        /// Out-of-range indices are clamped silently (matching `sed` behaviour).
        #[must_use]
        pub fn index_range(&self, total: usize) -> Range<usize> {
            match *self {
                Self::Single(m) => {
                    let idx = m.saturating_sub(1).min(total);
                    idx..idx.saturating_add(1).min(total)
                }
                Self::Range(start, end) => start.saturating_sub(1).min(total)..end.min(total),
                Self::Tail(n) => total.saturating_sub(n)..total,
            }
        }

        /// Apply this slice to a list of items, returning the selected sub-slice.
        #[must_use]
        pub fn apply<'a, T>(&self, items: &'a [T]) -> &'a [T] {
            let range = self.index_range(items.len());
            items.get(range).unwrap_or(&[])
        }
    }
}
