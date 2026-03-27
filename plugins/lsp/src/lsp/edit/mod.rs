//! Workspace edit utilities: apply LSP `WorkspaceEdit`s to files on disk,
//! or preview them as unified diffs without side effects.
//!
//! The core pipeline is shared between the two modes:
//! 1. Collect text edits from the `WorkspaceEdit` (both `changes` and
//!    `document_changes` fields).
//! 2. Read each affected file's content through the [`PathResolver`]
//!    (which rewrites FUSE paths to overlay paths, avoiding re-entrancy).
//! 3. Apply edits to a [`Rope`] in reverse position order so byte offsets
//!    remain stable across mutations.
//!
//! [`PathResolver`]: super::path::LspPathResolver

use std::collections::HashMap;
use std::path::Path;

use color_eyre::eyre::{Result, bail};
use crop::Rope;
use lsp_types::{DocumentChanges, OneOf, TextEdit, WorkspaceEdit};
use nyne::text;
use nyne::types::vfs_path::VfsPath;
use nyne_source::edit::plan::FileEditResult;
use tracing::{debug, warn};

use super::uri::position_to_byte_offset;

/// Apply a `WorkspaceEdit` to files on disk via the FUSE-safe `resolver`.
pub fn apply_workspace_edit(edit: &WorkspaceEdit, resolver: &super::path::PathResolver) -> Result<()> {
    let results = resolve_edits(edit, resolver)?;

    if results.is_empty() {
        warn!(
            target: "nyne::lsp",
            "workspace edit has zero changes — LSP server returned an empty edit",
        );
        return Ok(());
    }

    let total_edits: usize = results.iter().map(|r| r.edit_count).sum();
    debug!(
        target: "nyne::lsp",
        file_count = results.len(),
        total_edits,
        "applying workspace edit",
    );

    for result in &results {
        resolver.write_string(&result.path, &result.modified)?;
        debug!(
            target: "nyne::lsp",
            path = result.path,
            edit_count = result.edit_count,
            "applied workspace edits to file",
        );
    }

    Ok(())
}

/// Convert a `WorkspaceEdit` to a unified diff string without modifying any files.
pub fn workspace_edit_to_diff(edit: &WorkspaceEdit, resolver: &super::path::PathResolver) -> Result<String> {
    let mut results = resolve_edits(edit, resolver)?;

    if results.is_empty() {
        return Ok(String::new());
    }

    // Sort by path for deterministic output.
    results.sort_by(|a, b| a.path.cmp(&b.path));

    let mut diff = String::new();
    for result in &results {
        let display_path = resolver.rewrite_to_fuse(Path::new(&result.path));
        diff.push_str(&text::unified_diff(
            &result.original,
            &result.modified,
            &display_path.to_string_lossy(),
        ));
    }

    Ok(diff)
}

/// Collect all text edits from a `WorkspaceEdit`, grouped by file path.
///
/// Merges edits from both `changes` and `document_changes` into a single
/// map. The two fields are not mutually exclusive in the spec.
fn collect_all_edits(edit: &WorkspaceEdit) -> HashMap<String, Vec<&TextEdit>> {
    let mut file_edits: HashMap<String, Vec<&TextEdit>> = HashMap::new();

    if let Some(changes) = &edit.changes {
        for (uri, edits) in changes {
            collect_into(&mut file_edits, uri, edits.iter());
        }
    }

    if let Some(doc_changes) = &edit.document_changes {
        collect_document_changes(doc_changes, &mut file_edits);
    }

    file_edits
}

/// Add text edits for a single URI into the file edits map.
fn collect_into<'a>(
    file_edits: &mut HashMap<String, Vec<&'a TextEdit>>,
    uri: &lsp_types::Uri,
    edits: impl Iterator<Item = &'a TextEdit>,
) {
    file_edits
        .entry(super::uri::uri_to_file_path(uri).display().to_string())
        .or_default()
        .extend(edits);
}

/// Collect text edits from `DocumentChanges` into the file edits map.
fn collect_document_changes<'a>(doc_changes: &'a DocumentChanges, file_edits: &mut HashMap<String, Vec<&'a TextEdit>>) {
    let mut collect_edit = |edit: &'a lsp_types::TextDocumentEdit| {
        collect_into(
            file_edits,
            &edit.text_document.uri,
            edit.edits.iter().map(unwrap_text_edit),
        );
    };
    match doc_changes {
        DocumentChanges::Edits(edits) => edits.iter().for_each(&mut collect_edit),
        DocumentChanges::Operations(ops) => ops.iter().for_each(|op| {
            if let lsp_types::DocumentChangeOperation::Edit(edit) = op {
                collect_edit(edit);
            }
        }),
    }
}

/// Extract the underlying `TextEdit` from an `OneOf<TextEdit, AnnotatedTextEdit>`.
const fn unwrap_text_edit(edit: &OneOf<TextEdit, lsp_types::AnnotatedTextEdit>) -> &TextEdit {
    match edit {
        OneOf::Left(te) => te,
        OneOf::Right(annotated) => &annotated.text_edit,
    }
}

/// Apply text edits to a string via `Rope::replace`, returning the result.
///
/// Sorts edits in reverse position order so that each replacement doesn't
/// shift the byte offsets of subsequent ones. This is the shared core for
/// both `apply_workspace_edit` (writes to disk) and `workspace_edit_to_diff`
/// (preview without side effects).
fn apply_edits_to_rope(content: &str, edits: &mut [&TextEdit]) -> Result<String> {
    let mut rope = Rope::from(content);

    edits.sort_by(|a, b| {
        b.range
            .start
            .cmp(&a.range.start)
            .then_with(|| b.range.end.cmp(&a.range.end))
    });

    for edit in edits.iter() {
        let start = position_to_byte_offset(&rope, edit.range.start);
        let end = position_to_byte_offset(&rope, edit.range.end);
        match (start, end) {
            (Some(s), Some(e)) => {
                rope.replace(s..e, &edit.new_text);
            }
            _ => {
                bail!("workspace edit position out of range");
            }
        }
    }

    Ok(rope.to_string())
}

/// A file's original and modified content after applying text edits.
///
/// Intermediate result from [`resolve_edits`], consumed by both
/// [`apply_workspace_edit`] (writes `modified` to disk) and
/// [`workspace_edit_to_diff`] (diffs `original` vs `modified`).
struct ResolvedFileEdit {
    path: String,
    original: String,
    modified: String,
    edit_count: usize,
}

/// Collect, read, and apply all edits from a `WorkspaceEdit`, returning per-file results.
///
/// Shared pipeline for both `apply_workspace_edit` (write-back) and
/// `workspace_edit_to_diff` (preview). FUSE-safe: reads files through
/// `resolver`, which rewrites FUSE-based LSP paths to overlay paths.
fn resolve_edits(edit: &WorkspaceEdit, resolver: &super::path::PathResolver) -> Result<Vec<ResolvedFileEdit>> {
    let file_edits = collect_all_edits(edit);

    file_edits
        .into_iter()
        .map(|(path, mut edits)| {
            let edit_count = edits.len();
            let original = resolver.read_to_string(&path)?;
            let modified = apply_edits_to_rope(&original, &mut edits)?;
            Ok(ResolvedFileEdit {
                path,
                original,
                modified,
                edit_count,
            })
        })
        .collect()
}

/// Resolve a `WorkspaceEdit` into [`FileEditResult`]s for use with [`DiffAction`].
///
/// Handles both text edits (via [`resolve_edits`]) and file-level operations
/// (`CreateFile`, `RenameFile`, `DeleteFile`) from `DocumentChanges::Operations`.
///
/// Path translation is encapsulated: absolute overlay paths from LSP are
/// converted to relative `VfsPath`s, and display paths use the FUSE root.
///
/// [`DiffAction`]: crate::edit::diff_action::DiffAction
pub fn resolve_workspace_edit(
    edit: &WorkspaceEdit,
    resolver: &super::path::PathResolver,
) -> Result<Vec<FileEditResult>> {
    use nyne_source::edit::plan::EditOutcome;

    // Convert an absolute path to a (VfsPath, display_path) pair.
    let to_paths = |abs: &str| -> Result<(VfsPath, String)> {
        let rel = Path::new(abs)
            .strip_prefix(resolver.overlay_root())
            .map_or_else(|_| abs.to_owned(), |p| p.to_string_lossy().into_owned());
        let display = resolver.rewrite_to_fuse(Path::new(abs)).to_string_lossy().into_owned();
        Ok((VfsPath::new(&rel)?, display))
    };

    let mut results: Vec<FileEditResult> = Vec::new();

    // 1. Resolve text edits into Modify results.
    for r in resolve_edits(edit, resolver)? {
        let (source_file, display_path) = to_paths(&r.path)?;
        results.push(FileEditResult::skipped(
            source_file,
            display_path,
            r.original,
            r.modified,
            EditOutcome::Modify,
        ));
    }

    // 2. Collect file-level operations from document_changes.
    if let Some(DocumentChanges::Operations(ops)) = &edit.document_changes {
        for op in ops {
            let lsp_types::DocumentChangeOperation::Op(resource_op) = op else {
                continue; // Text edits already handled above.
            };
            collect_resource_op(resource_op, resolver, &to_paths, &mut results)?;
        }
    }

    // Sort by display path for deterministic output.
    results.sort_by(|a, b| a.display_path.cmp(&b.display_path));

    Ok(results)
}

/// Process a single `ResourceOp` (Create, Delete, or Rename) into the results list.
///
/// Resource ops may coexist with text edits for the same file. When a
/// `Create` or `Rename` targets a file that already has text edits in
/// `results`, the existing entry's outcome is updated rather than
/// duplicated. `Delete` supersedes any prior edits for the same path.
fn collect_resource_op(
    op: &lsp_types::ResourceOp,
    resolver: &super::path::PathResolver,
    to_paths: &impl Fn(&str) -> Result<(VfsPath, String)>,
    results: &mut Vec<FileEditResult>,
) -> Result<()> {
    use nyne_source::edit::plan::EditOutcome;

    match op {
        lsp_types::ResourceOp::Create(create) => {
            let (vfs, display) = to_paths(create.uri.path().as_str())?;
            if let Some(existing) = results.iter_mut().find(|r| r.display_path == display) {
                existing.outcome = EditOutcome::Create;
            } else {
                results.push(FileEditResult::skipped(
                    vfs,
                    display,
                    String::new(),
                    String::new(),
                    EditOutcome::Create,
                ));
            }
        }
        lsp_types::ResourceOp::Delete(delete) => {
            let (vfs, display) = to_paths(delete.uri.path().as_str())?;
            results.retain(|r| r.display_path != display);
            results.push(FileEditResult::skipped(
                vfs,
                display,
                String::new(),
                String::new(),
                EditOutcome::Delete,
            ));
        }
        lsp_types::ResourceOp::Rename(rename) => {
            let (old_vfs, old_display) = to_paths(rename.old_uri.path().as_str())?;
            let (new_vfs, _) = to_paths(rename.new_uri.path().as_str())?;
            if let Some(existing) = results.iter_mut().find(|r| r.display_path == old_display) {
                existing.outcome = EditOutcome::Rename { new_path: new_vfs };
            } else {
                let content = resolver.read_to_string(rename.old_uri.path().as_str())?;
                results.push(FileEditResult::skipped(
                    old_vfs,
                    old_display,
                    content.clone(),
                    content,
                    EditOutcome::Rename { new_path: new_vfs },
                ));
            }
        }
    }
    Ok(())
}

/// Unit tests.
#[cfg(test)]
mod tests;
