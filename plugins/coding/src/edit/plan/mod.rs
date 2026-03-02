//! File edit operations and planning.

use std::ops::Range;

use color_eyre::eyre::Result;
use strum::IntoEnumIterator;

use super::splice::splice_content;
use crate::syntax::fragment::Fragment;

/// Fieldless discriminant for [`EditOp`] — the single source of truth for
/// the set of edit operation kinds.
///
/// Used for filesystem anchors (directory names, labels, parsing) and
/// iteration. Every `EditOp` variant has a corresponding `EditOpKind`.
///
/// The `kebab-case` serialization is used for both VFS directory names
/// and staged action labels — adding a variant requires no manual name
/// mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr, strum::EnumIter, strum::EnumCount)]
#[strum(serialize_all = "kebab-case")]
pub enum EditOpKind {
    Replace,
    Delete,
    InsertBefore,
    InsertAfter,
    Append,
}

impl EditOpKind {
    /// Kebab-case name, used as both VFS directory name and staged action label.
    #[must_use]
    pub fn name(self) -> &'static str { self.into() }

    /// Parse a directory/label name back to an operation kind.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> { Self::iter().find(|k| k.name() == name) }
}

/// A single edit operation targeting a source file.
///
/// `#[non_exhaustive]` — future variants (e.g., `AstGrep` for structural
/// pattern matching) can be added without breaking downstream matches.
#[non_exhaustive]
#[derive(Clone)]
pub enum EditOp {
    /// Replace a symbol's body with new content.
    Replace {
        /// Fragment path (e.g., `["Foo", "bar"]` for nested symbol).
        fragment_path: Vec<String>,
        /// New content to splice in.
        content: String,
    },

    /// Delete a symbol entirely (including decorators, docstring, signature).
    Delete { fragment_path: Vec<String> },

    /// Insert content before a symbol.
    InsertBefore {
        fragment_path: Vec<String>,
        content: String,
    },

    /// Insert content after a symbol.
    InsertAfter {
        fragment_path: Vec<String>,
        content: String,
    },

    /// Append content as last child of a scope.
    /// Rejects leaf symbols (empty `children`) with EINVAL.
    Append {
        fragment_path: Vec<String>,
        content: String,
    },
}

/// An `EditOp` resolved to a concrete byte range in the source.
pub struct ResolvedEdit {
    /// The original staged order (user numbering).
    pub staged_index: u32,
    /// Byte range in the original source to replace.
    pub byte_range: Range<usize>,
    /// Replacement content (empty string for deletions).
    pub replacement: String,
}

/// A plan of edits for a single source file.
pub struct EditPlan {
    pub ops: Vec<(u32, EditOp)>,
}

impl EditOp {
    /// The discriminant kind of this operation.
    #[must_use]
    pub const fn kind(&self) -> EditOpKind {
        match self {
            Self::Replace { .. } => EditOpKind::Replace,

            Self::Delete { .. } => EditOpKind::Delete,
            Self::InsertBefore { .. } => EditOpKind::InsertBefore,
            Self::InsertAfter { .. } => EditOpKind::InsertAfter,
            Self::Append { .. } => EditOpKind::Append,
        }
    }

    /// The content payload, if any. `Delete` has no content.
    #[must_use]
    pub fn content(&self) -> &str {
        match self {
            Self::Replace { content, .. }
            | Self::InsertBefore { content, .. }
            | Self::InsertAfter { content, .. }
            | Self::Append { content, .. } => content,
            Self::Delete { .. } => "",
        }
    }

    /// Replace the content payload. No-op for `Delete`.
    pub fn set_content(&mut self, new_content: String) {
        match self {
            Self::Replace { content, .. }
            | Self::InsertBefore { content, .. }
            | Self::InsertAfter { content, .. }
            | Self::Append { content, .. } => *content = new_content,
            Self::Delete { .. } => {}
        }
    }
}

impl EditPlan {
    /// Resolve all edit ops to concrete byte ranges in the source.
    ///
    /// Navigates the fragment tree for each op, computes the target byte
    /// range, checks for overlapping edits, and returns resolved edits
    /// sorted bottom-up (descending byte offset) for safe application.
    pub fn resolve(&self, fragments: &[Fragment], source: &str) -> Result<Vec<ResolvedEdit>> {
        use crate::edit::splice::{extend_delete_range, line_start_of};
        use crate::syntax::require_fragment;

        let mut resolved = Vec::with_capacity(self.ops.len());

        for (index, op) in &self.ops {
            let edit = match op {
                EditOp::Replace { fragment_path, content } => {
                    let frag = require_fragment(fragments, fragment_path)?;
                    // Use full_span (decorators + docstring + signature + body)
                    // to match body.rs read range — ensures round-trip:
                    // `cat body.rs > edit/replace` is a no-op.
                    let start = line_start_of(source, frag.full_span.start);
                    ResolvedEdit {
                        staged_index: *index,
                        byte_range: start..frag.full_span.end,
                        replacement: content.clone(),
                    }
                }
                EditOp::Delete { fragment_path } => {
                    let frag = require_fragment(fragments, fragment_path)?;
                    let range = extend_delete_range(source, &frag.full_span);
                    ResolvedEdit {
                        staged_index: *index,
                        byte_range: range,
                        replacement: String::new(),
                    }
                }
                EditOp::InsertBefore { fragment_path, content } => {
                    let frag = require_fragment(fragments, fragment_path)?;
                    let offset = line_start_of(source, frag.full_span.start);
                    // Ensure trailing newline so the inserted content doesn't
                    // join directly to the anchor symbol's first line.
                    let replacement = ensure_trailing_newline(content);
                    ResolvedEdit {
                        staged_index: *index,
                        byte_range: offset..offset,
                        replacement,
                    }
                }
                EditOp::InsertAfter { fragment_path, content } => {
                    let frag = require_fragment(fragments, fragment_path)?;
                    let offset = frag.full_span.end;
                    // Ensure leading newline so inserted content doesn't join
                    // directly to the anchor symbol's closing delimiter.
                    let replacement = ensure_leading_newline(source, offset, content);
                    ResolvedEdit {
                        staged_index: *index,
                        byte_range: offset..offset,
                        replacement,
                    }
                }
                EditOp::Append { fragment_path, content } => {
                    let frag = require_fragment(fragments, fragment_path)?;
                    // Append after the last child, or inside the empty scope body
                    // (just before the closing brace).
                    let offset = append_offset(source, frag);
                    // Ensure leading newline so appended content is separated.
                    let replacement = ensure_leading_newline(source, offset, content);
                    ResolvedEdit {
                        staged_index: *index,
                        byte_range: offset..offset,
                        replacement,
                    }
                }
                // Future variants (e.g., AstGrep) — `non_exhaustive` requires this arm.
                #[allow(unreachable_patterns)]
                _ => continue,
            };
            resolved.push(edit);
        }

        // Detect overlapping non-empty ranges (conflicts).
        Self::check_conflicts(&resolved)?;

        // Sort by byte_range.start descending for bottom-up application.
        resolved.sort_by(|a, b| b.byte_range.start.cmp(&a.byte_range.start));

        Ok(resolved)
    }

    /// Check for overlapping edit ranges.
    fn check_conflicts(edits: &[ResolvedEdit]) -> Result<()> {
        let mut sorted: Vec<&ResolvedEdit> = edits.iter().collect();
        sorted.sort_by_key(|e| e.byte_range.start);

        for pair in sorted.windows(2) {
            let &[a, b] = pair else { continue };
            // Two zero-width insertions at the same point are fine.
            if a.byte_range.is_empty() && b.byte_range.is_empty() {
                continue;
            }
            if a.byte_range.end > b.byte_range.start {
                return Err(color_eyre::eyre::eyre!(
                    "conflicting edits: action {} (bytes {}..{}) overlaps with action {} (bytes {}..{})",
                    a.staged_index,
                    a.byte_range.start,
                    a.byte_range.end,
                    b.staged_index,
                    b.byte_range.start,
                    b.byte_range.end,
                ));
            }
        }
        Ok(())
    }

    /// Apply resolved edits to source text, returning the modified result.
    ///
    /// Edits must be sorted by `byte_range.start` descending (bottom-up)
    /// so that earlier byte offsets remain valid as later regions are spliced.
    #[must_use]
    pub fn apply(source: &str, edits: &[ResolvedEdit]) -> String {
        let mut result = source.to_owned();
        for edit in edits {
            result = splice_content(&result, edit.byte_range.clone(), &edit.replacement);
        }
        result
    }
}

// Re-exported from `nyne::edit` (SSOT lives in core — `node::diff_action` needs them).
pub use nyne::edit::{EditOutcome, FileEditResult, ValidationResult, apply_file_edits};

/// Compute the byte offset for an `Append` edit within a fragment.
///
/// If the fragment has children, appends after the last child's span.
/// Otherwise, finds the closing brace and inserts before it.
fn append_offset(source: &str, frag: &Fragment) -> usize {
    if let Some(last) = frag.children.last() {
        return last.full_span.end;
    }
    // Empty scope: find closing brace and insert before it.
    source[frag.byte_range.start..frag.byte_range.end]
        .rfind('}')
        .map_or(frag.byte_range.end, |pos| frag.byte_range.start + pos)
}

/// Ensure content has a leading newline separator when the source at `offset`
/// doesn't already end with one. Prevents inserted/appended content from joining
/// directly to the previous symbol's closing delimiter.
fn ensure_leading_newline(source: &str, offset: usize, content: &str) -> String {
    let prev_is_newline = offset > 0 && source.as_bytes().get(offset - 1) == Some(&b'\n');
    let content_starts_newline = content.starts_with('\n');
    if prev_is_newline || content_starts_newline {
        content.to_owned()
    } else {
        format!("\n{content}")
    }
}

/// Ensure content has a trailing newline so it doesn't join directly to the
/// following symbol's first line.
fn ensure_trailing_newline(content: &str) -> String {
    if content.ends_with('\n') {
        content.to_owned()
    } else {
        format!("{content}\n")
    }
}

#[cfg(test)]
mod tests;
