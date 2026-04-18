//! Diff middleware provider.
//!
//! Intercepts lookup and remove operations for paths where a downstream
//! producer has set [`DiffCapable`] request state. On lookup, creates a
//! file node with a unified diff preview. On remove, applies the edits.

pub mod state;
use std::iter;
use std::sync::Arc;

use color_eyre::eyre::{Result, WrapErr, eyre};
use nyne::router::{
    AffectedFiles, CachePolicy, Filesystem, Next, Node, Op, Provider, ReadContext, Readable, Request, UnlinkContext,
    Unlinkable,
};
use nyne::text::unified_diff;
pub use state::*;
use tracing::debug;

/// Diff middleware — creates preview nodes on lookup, applies edits on remove.
pub struct DiffProvider {
    pub(crate) root_prefix: String,
}

nyne::define_provider!(DiffProvider, "diff", priority: -40);

/// Sentinel content returned when a diff action produces no edits.
const NO_CHANGES: &[u8] = b"No changes.\n";

impl Provider for DiffProvider {
    fn accept(&self, req: &mut Request, next: &Next) -> Result<()> {
        let result = next.run(req);

        let Some(diff) = req.state::<DiffCapable>().cloned() else {
            return result;
        };

        match req.op().clone() {
            Op::Lookup { ref name } => {
                result?;
                debug!(target: "nyne::diff", name, "creating diff preview node");
                req.nodes.add(
                    Node::file()
                        .with_readable(DiffPreview {
                            source: Arc::clone(&diff.source),
                            root_prefix: self.root_prefix.clone(),
                        })
                        .with_cache_policy(CachePolicy::NoCache)
                        .named(name),
                );
                Ok(())
            }
            Op::Remove { ref name } => {
                debug!(target: "nyne::diff", name, "applying diff");
                apply_diff(&diff)?;
                Ok(())
            }
            _ => result,
        }
    }
}

/// Apply a [`DiffCapable`] — compute edits, validate, write to disk, run post-apply hook.
fn apply_diff(diff: &DiffCapable) -> Result<()> {
    let edits = diff.source.compute_edits().wrap_err("failed to compute edits")?;

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

    apply_file_edits(&edits, diff.fs.as_ref()).wrap_err("failed to apply edits")?;
    diff.source.on_applied().wrap_err("post-apply hook failed")?;
    Ok(())
}

/// `Unlinkable` wrapper for attaching diff-based delete to non-`.diff` nodes.
///
/// Used for `rmdir Foo@/` (symbol deletion) where the directory node needs
/// an `Unlinkable` capability backed by a [`DiffSource`].
pub struct DiffUnlinkable {
    source: Arc<dyn DiffSource>,
    fs: Arc<dyn Filesystem>,
}

impl DiffUnlinkable {
    pub fn new(source: impl DiffSource + 'static, fs: Arc<dyn Filesystem>) -> Self {
        Self {
            source: Arc::new(source),
            fs,
        }
    }
}

impl Unlinkable for DiffUnlinkable {
    fn unlink(&self, _ctx: &UnlinkContext<'_>) -> Result<AffectedFiles> {
        apply_diff(&DiffCapable {
            source: Arc::clone(&self.source),
            fs: Arc::clone(&self.fs),
        })?;
        Ok(Vec::new())
    }
}

/// Readable that renders a [`DiffSource`] as a unified diff preview.
struct DiffPreview {
    source: Arc<dyn DiffSource>,
    root_prefix: String,
}

impl Readable for DiffPreview {
    fn read(&self, ctx: &ReadContext<'_>) -> Result<Vec<u8>> {
        let edits = self.source.compute_edits().wrap_err("failed to compute diff preview")?;

        if edits.is_empty() {
            return Ok(NO_CHANGES.to_vec());
        }

        let mut header_lines = self.source.header_lines();
        header_lines.extend(validation_header_lines(&edits));
        header_lines.push(format!("To apply: rm {}", ctx.path.display()));

        Ok(iter::once(format_header(&header_lines))
            .chain(edits.iter().map(|e| format_edit(e, &self.root_prefix)))
            .collect::<String>()
            .into_bytes())
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
///
/// Dispatches on the edit outcome: deletions and renames become comment lines,
/// creates and modifications become unified diffs. Renames include both the
/// comment header and a diff of the content change.
///
/// `root_prefix` is stripped from display paths so that diff headers use
/// project-relative paths compatible with `patch -p1`.
fn format_edit(edit: &FileEditResult, root_prefix: &str) -> String {
    let display = strip_root_prefix(&edit.display_path, root_prefix);
    match &edit.outcome {
        EditOutcome::Delete => ["# Deleted: ", display, "\n"].concat(),
        EditOutcome::Create => unified_diff("", &edit.modified, display),
        EditOutcome::Rename { new_path } => {
            let new_path_str = new_path.display().to_string();
            let new_display = strip_root_prefix(&new_path_str, root_prefix);
            let mut out = ["# Renamed: ", display, " -> ", new_display, "\n"].concat();
            out.push_str(&unified_diff(&edit.original, &edit.modified, new_display));
            out
        }
        EditOutcome::Modify => unified_diff(&edit.original, &edit.modified, display),
    }
}

/// Strip a root prefix from a path to produce a project-relative path.
///
/// This is the single location where absolute display paths are normalized
/// for diff output. All diff rendering flows through [`format_edit`], which
/// calls this before passing paths to [`unified_diff`].
fn strip_root_prefix<'a>(path: &'a str, root_prefix: &str) -> &'a str { path.strip_prefix(root_prefix).unwrap_or(path) }

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
