//! Shared trait for types that compute file edits for preview and application.
//!
//! Implements the diff-preview-then-apply-on-delete pattern used by code
//! actions, symbol deletion, and batch edits. A single `compute_edits()`
//! method is the SSOT — [`DiffActionNode`] derives both `Readable` (preview
//! as unified diff) and `Unlinkable` (apply edits to disk) from it.

use std::iter;

use color_eyre::eyre::{Result, eyre};
use nyne::RequestContext;
use nyne::format::unified_diff;
use nyne::node::capabilities::{Readable, Unlinkable};

use super::plan::{EditOutcome, FileEditResult, ValidationResult, apply_file_edits};

/// Sentinel content returned when a diff action produces no edits.
const NO_CHANGES: &[u8] = b"No changes.\n";

/// Trait for types that compute file edits for preview and application.
///
/// Implementors produce a list of per-file edit results. The [`DiffActionNode`]
/// wrapper then generates unified diffs for preview (`Readable`) and writes
/// modified content to disk on delete (`Unlinkable`).
pub trait DiffAction: Send + Sync {
    /// Compute the edits this action would perform.
    ///
    /// Returns a list of per-file results with original and modified content.
    fn compute_edits(&self, ctx: &RequestContext<'_>) -> Result<Vec<FileEditResult>>;

    /// Header lines describing this diff action.
    ///
    /// Each line is automatically prefixed with `# ` (diff comment syntax).
    /// An empty vec produces no header. A trailing `#\n` separator is appended
    /// after the last line.
    fn header_lines(&self) -> Vec<String> { Vec::new() }

    /// Post-apply hook — called after edits are successfully written to disk.
    ///
    /// Default is a no-op. Override to clear staging areas, invalidate caches, etc.
    fn on_applied(&self, _ctx: &RequestContext<'_>) -> Result<()> { Ok(()) }
}

/// Wrapper that derives `Readable` (diff preview) and `Unlinkable` (apply)
/// from any [`DiffAction`] implementor.
///
/// Captures the node's filename at construction so the apply instruction
/// in the diff header shows the actual VFS path (constructed from
/// `ctx.path` + `name` at read time).
///
/// Usage in provider code:
/// ```ignore
/// let action = MyAction { ... };
/// DiffActionNode::into_node("preview.diff", action);
/// ```
pub struct DiffActionNode<T> {
    action: T,
    name: String,
}

impl<T> DiffActionNode<T> {
    /// Wrap an action with the node's filename for path-aware headers.
    pub fn new(name: impl Into<String>, action: T) -> Self {
        Self {
            action,
            name: name.into(),
        }
    }
}

impl<T: DiffAction + Clone + 'static> DiffActionNode<T> {
    /// Create a file node with both diff preview (`Readable`) and apply-on-delete (`Unlinkable`).
    pub fn into_node(name: impl Into<String>, action: T) -> nyne::VirtualNode {
        let name = name.into();
        nyne::VirtualNode::file(&name, Self::new(&name, action.clone())).with_unlinkable(Self::new(&name, action))
    }
}

impl<T: DiffAction> Readable for DiffActionNode<T> {
    fn read(&self, ctx: &RequestContext<'_>) -> Result<Vec<u8>> {
        let edits = self.action.compute_edits(ctx)?;

        if edits.is_empty() {
            return Ok(NO_CHANGES.to_vec());
        }

        let mut header_lines = self.action.header_lines();
        header_lines.extend(validation_header_lines(&edits));

        // Auto-append apply instruction with the actual VFS path.
        let apply_path = ctx
            .path
            .join(&self.name)
            .map_or_else(|_| self.name.clone(), |p| p.to_string());
        header_lines.push(format!("To apply: rm {apply_path}"));

        let output: String = iter::once(format_header(&header_lines))
            .chain(edits.iter().map(format_edit))
            .collect();

        Ok(if output.is_empty() {
            NO_CHANGES.to_vec()
        } else {
            output.into_bytes()
        })
    }
}

impl<T: DiffAction> Unlinkable for DiffActionNode<T> {
    fn unlink(&self, ctx: &RequestContext<'_>) -> Result<()> {
        let edits = self.action.compute_edits(ctx)?;

        if edits.is_empty() {
            return Err(eyre!("no edits to apply"));
        }

        let failures: Vec<&str> = edits
            .iter()
            .filter_map(|e| match &e.validation {
                ValidationResult::Fail(msg) => Some(msg.as_str()),
                ValidationResult::Pass | ValidationResult::Skipped => None,
            })
            .collect();
        if !failures.is_empty() {
            return Err(eyre!("validation failed, refusing to apply:\n{}", failures.join("\n"),));
        }

        apply_file_edits(&edits, ctx.real_fs)?;
        self.action.on_applied(ctx)?;
        Ok(())
    }
}

/// Format header lines as diff comments (`# ` prefix per line).
///
/// Appends a bare `#` separator after the last line. Returns an empty
/// string when `lines` is empty.
fn format_header(lines: &[String]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for line in lines {
        out.push_str("# ");
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("#\n");
    out
}

/// Format a single [`FileEditResult`] as a diff or comment string.
fn format_edit(edit: &FileEditResult) -> String {
    match &edit.outcome {
        EditOutcome::Delete => format!("# Deleted: {}\n", edit.display_path),
        EditOutcome::Create => unified_diff("", &edit.modified, &edit.display_path),
        EditOutcome::Rename { new_path } => {
            let mut out = format!("# Renamed: {} -> {}\n", edit.display_path, new_path);
            out.push_str(&unified_diff(&edit.original, &edit.modified, new_path.as_str()));
            out
        }
        EditOutcome::Modify => unified_diff(&edit.original, &edit.modified, &edit.display_path),
    }
}

/// Build validation summary lines for the diff header.
///
/// Returns an empty iterator if all edits are `Skipped` (no validation performed).
fn validation_header_lines(edits: &[FileEditResult]) -> impl Iterator<Item = String> + '_ {
    edits
        .iter()
        .any(|e| !matches!(e.validation, ValidationResult::Skipped))
        .then(|| {
            if edits.iter().any(|e| matches!(e.validation, ValidationResult::Fail(_))) {
                "Validation: FAIL".to_owned()
            } else {
                "Validation: PASS".to_owned()
            }
        })
        .into_iter()
        .chain(edits.iter().filter_map(|e| match &e.validation {
            ValidationResult::Fail(msg) => Some(format!("Error: {msg}")),
            _ => None,
        }))
}
