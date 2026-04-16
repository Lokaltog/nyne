//! Edit plan resolution and application.
//!
//! An [`EditPlan`] collects staged [`EditOp`]s for a single source file,
//! resolves them to concrete byte ranges via the fragment tree, checks for
//! conflicts, and applies them in reverse order to avoid offset invalidation.
//! Tree-sitter validation ensures the result parses cleanly before write-back.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::ops::Range;

use color_eyre::eyre::Result;

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
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr, strum::EnumIter, strum::EnumCount, strum::EnumString,
)]
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
}

/// A single edit operation targeting a source file.
///
/// Each op targets a `fragment_path` (symbol address in the tree) with a
/// [`kind`](EditOpKind) and optional `content`. Only [`Delete`](EditOpKind::Delete)
/// carries no content.
#[derive(Clone)]
pub struct EditOp {
    /// Fragment path (e.g., `["Foo", "bar"]` for nested symbol).
    pub fragment_path: Vec<String>,
    /// The operation to perform on the target symbol.
    pub kind: EditOpKind,
    /// Content payload. `None` only for `Delete`.
    pub content: Option<String>,
}

/// An [`EditOp`] resolved to a concrete byte range in the source.
///
/// Produced by [`EditPlan::resolve`], which walks the decomposed symbol tree
/// to translate fragment paths into byte ranges. Resolved edits are then
/// sorted and conflict-checked before being applied in reverse order to
/// avoid offset invalidation.
pub struct ResolvedEdit {
    /// The original staged order (user numbering).
    pub staged_index: u32,
    /// Byte range in the original source to replace.
    pub byte_range: Range<usize>,
    /// Replacement content (empty string for deletions).
    pub replacement: String,
}
impl ResolvedEdit {
    /// Ascending order by `byte_range.start`, with zero-width insertions
    /// sorting before non-empty edits (replacements/deletions) at the same
    /// offset so insertions at a boundary are processed first.
    fn cmp_ascending(&self, other: &Self) -> Ordering {
        self.byte_range
            .start
            .cmp(&other.byte_range.start)
            .then_with(|| self.byte_range.is_empty().cmp(&other.byte_range.is_empty()).reverse())
    }
}

/// A plan of edits for a single source file.
///
/// Collects staged [`EditOp`]s paired with their user-assigned sequence
/// numbers (the `u32`). Call [`resolve`](Self::resolve) to translate
/// fragment paths into byte ranges, then [`apply`](Self::apply) to
/// produce the modified source text.
pub struct EditPlan {
    /// Staged operations in user-assigned order. The `u32` is the staged
    /// sequence number used for conflict reporting and diff labels.
    pub ops: Vec<(u32, EditOp)>,
}

impl EditOp {
    /// The content payload, or `""` when absent (`Delete`).
    #[must_use]
    pub fn content(&self) -> &str { self.content.as_deref().unwrap_or("") }

    /// Replace the content payload. No-op for `Delete`.
    pub fn set_content(&mut self, new_content: String) {
        if self.content.is_some() {
            self.content = Some(new_content);
        }
    }
}

impl EditPlan {
    /// Resolve all edit ops to concrete byte ranges in the source.
    ///
    /// Navigates the fragment tree for each op, computes the target byte
    /// range, detects overlapping edits, and returns resolved edits sorted
    /// ascending by byte offset for single-pass application via
    /// [`apply`](Self::apply).
    pub fn resolve(&self, fragments: &[Fragment], source: &str) -> Result<Vec<ResolvedEdit>> {
        use crate::edit::splice::{extend_delete_range, line_start_of_rope};
        use crate::syntax::require_fragment;

        let rope = crop::Rope::from(source);
        let mut resolved = Vec::with_capacity(self.ops.len());

        for (index, op) in &self.ops {
            let frag = require_fragment(fragments, &op.fragment_path)?;
            let content = op.content();

            let edit = match op.kind {
                EditOpKind::Replace => {
                    // Use full_span (decorators + docstring + signature + body)
                    // to match body.rs read range — ensures round-trip:
                    // `cat body.rs > edit/replace` is a no-op.
                    let span = frag.full_span();
                    let start = line_start_of_rope(&rope, span.start);
                    ResolvedEdit {
                        staged_index: *index,
                        byte_range: start..span.end,
                        replacement: content.to_owned(),
                    }
                }
                EditOpKind::Delete => {
                    let range = extend_delete_range(source, frag.full_span());
                    ResolvedEdit {
                        staged_index: *index,
                        byte_range: range,
                        replacement: String::new(),
                    }
                }
                EditOpKind::InsertBefore => {
                    let offset = line_start_of_rope(&rope, frag.full_span().start);
                    // Ensure trailing newline so the inserted content doesn't
                    // join directly to the anchor symbol's first line.
                    let replacement = ensure_trailing_newline(content).into_owned();
                    ResolvedEdit {
                        staged_index: *index,
                        byte_range: offset..offset,
                        replacement,
                    }
                }
                EditOpKind::InsertAfter => {
                    let offset = frag.full_span().end;
                    // Ensure leading newline so inserted content doesn't join
                    // directly to the anchor symbol's closing delimiter.
                    let replacement = ensure_leading_newline(source, offset, content).into_owned();
                    ResolvedEdit {
                        staged_index: *index,
                        byte_range: offset..offset,
                        replacement,
                    }
                }
                EditOpKind::Append => {
                    // Append after the last child, or inside the empty scope body
                    // (just before the closing brace).
                    let offset = append_offset(source, frag);
                    // Ensure leading newline so appended content is separated.
                    let replacement = ensure_leading_newline(source, offset, content).into_owned();
                    ResolvedEdit {
                        staged_index: *index,
                        byte_range: offset..offset,
                        replacement,
                    }
                }
            };
            resolved.push(edit);
        }

        // Sort ascending by byte offset (insertions before non-empty edits
        // at the same offset), then detect overlaps by walking adjacent pairs.
        resolved.sort_by(ResolvedEdit::cmp_ascending);
        Self::check_conflicts(&resolved)?;
        Ok(resolved)
    }

    /// Check for overlapping edit ranges — `edits` must be pre-sorted ascending.
    fn check_conflicts(edits: &[ResolvedEdit]) -> Result<()> {
        for pair in edits.windows(2) {
            let [a, b] = pair else { continue };
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
    /// Edits must be pre-sorted ascending by `byte_range.start` (as produced
    /// by [`resolve`](Self::resolve)) so a single O(L) pass copies the
    /// source in order, splicing each edit at its target range.
    #[must_use]
    pub fn apply(source: &str, edits: &[ResolvedEdit]) -> String {
        let mut result = String::with_capacity(source.len());
        let mut cursor = 0;
        for edit in edits {
            result.push_str(&source[cursor..edit.byte_range.start]);
            result.push_str(&edit.replacement);
            cursor = edit.byte_range.end;
        }
        result.push_str(&source[cursor..]);
        result
    }

    /// Resolve, apply, and validate this plan against a decomposed source,
    /// returning a [`FileEditResult`] describing the edit.
    ///
    /// SSOT for the resolve → apply → validate pipeline used by both
    /// batch edit staging ([`BatchEditAction`](crate::edit::staging::BatchEditAction))
    /// and symbol deletion previews.
    ///
    /// The caller supplies the validation result — callers that can't or
    /// don't need to validate pass [`ValidationResult::Pass`] or
    /// [`ValidationResult::Skipped`] directly rather than building a
    /// validator closure.
    pub fn run(
        &self,
        parsed: &crate::syntax::decomposed::DecomposedSource,
        source_file: std::path::PathBuf,
        validate: impl FnOnce(&str) -> nyne_diff::ValidationResult,
    ) -> Result<nyne_diff::FileEditResult> {
        let resolved = self.resolve(&parsed.decomposed, &parsed.source)?;
        let modified = Self::apply(&parsed.source, &resolved);
        let validation = validate(&modified);
        Ok(nyne_diff::FileEditResult {
            display_path: source_file.display().to_string(),
            source_file,
            original: parsed.source.clone(),
            modified,
            outcome: nyne_diff::EditOutcome::Modify,
            validation,
        })
    }
}

/// Compute the byte offset for an `Append` edit within a fragment.
///
/// If the fragment has children, appends after the last child's span.
/// Otherwise, finds the closing brace and inserts before it.
fn append_offset(source: &str, frag: &Fragment) -> usize {
    if let Some(last) = frag.children.last() {
        return last.span.full_span.end;
    }
    // Empty scope: find closing brace and insert before it.
    let body = &frag.span.byte_range;
    source[body.start..body.end]
        .rfind('}')
        .map_or(body.end, |pos| body.start + pos)
}

/// Ensure content has a leading newline separator when the source at `offset`
/// doesn't already end with one. Prevents inserted/appended content from joining
/// directly to the previous symbol's closing delimiter.
fn ensure_leading_newline<'a>(source: &str, offset: usize, content: &'a str) -> Cow<'a, str> {
    let prev_is_newline = offset > 0 && source.as_bytes().get(offset - 1) == Some(&b'\n');
    if prev_is_newline || content.starts_with('\n') {
        Cow::Borrowed(content)
    } else {
        Cow::Owned(format!("\n{content}"))
    }
}

/// Ensure content has a trailing newline so it doesn't join directly to the
/// following symbol's first line.
fn ensure_trailing_newline(content: &str) -> Cow<'_, str> {
    if content.ends_with('\n') {
        Cow::Borrowed(content)
    } else {
        Cow::Owned(format!("{content}\n"))
    }
}

#[cfg(test)]
mod tests;
