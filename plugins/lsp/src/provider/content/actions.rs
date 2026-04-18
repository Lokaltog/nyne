//! LSP code action content — [`DiffSource`] for preview and apply.
//!
//! Each code action is exposed as a virtual diff file under `symbols/Foo@/actions/`.
//! Reading the file previews the edit; deleting it applies the action via the diff middleware.

use std::ops::Range as StdRange;
use std::sync::OnceLock;

use color_eyre::eyre::{Result, eyre};
use lsp_types::CodeAction;
use nyne::router::NamedNode;
use nyne::text;
use nyne_diff::{DiffSource, FileEditResult};

use crate::session::edit::resolve_workspace_edit;
use crate::session::handle::LspQuery;
use crate::session::uri::line_range_to_lsp_range;

/// Maximum length for the kebab-case slug derived from a code action title.
const ACTION_SLUG_MAX_LEN: usize = 60;

/// A resolved code action with its display index and generated filename.
pub struct ResolvedAction {
    pub action: CodeAction,
    pub file_name: String,
}

/// Query code actions for a symbol and return them as resolved entries.
///
/// Called at resolve time (readdir of `actions/`) to eagerly fetch
/// the list of available actions from the LSP server.
pub fn resolve_code_actions(query: &LspQuery, line_range: &StdRange<usize>) -> Vec<ResolvedAction> {
    let Some(fq) = query.file_query() else {
        return Vec::new();
    };

    let actions = match fq.code_actions(line_range_to_lsp_range(line_range)) {
        Ok(actions) => actions,
        Err(err) => {
            tracing::debug!(?err, "code action query failed");
            return Vec::new();
        }
    };

    actions
        .into_iter()
        .filter(|a| a.disabled.is_none())
        .enumerate()
        .map(|(i, action)| {
            let slug = text::slugify(&action.title, ACTION_SLUG_MAX_LEN);
            let file_name = format!("{:02}-{slug}.diff", i + 1);
            ResolvedAction { action, file_name }
        })
        .collect()
}

/// Build bare file nodes for resolved code actions (readdir).
///
/// Each action becomes a named file entry. The diff middleware handles
/// preview and apply — callers set [`DiffCapable`] on lookup/remove via
/// [`find_action_diff`].
pub fn build_action_nodes(resolved: &[ResolvedAction]) -> Vec<NamedNode> {
    resolved.iter().map(|entry| NamedNode::file(&entry.file_name)).collect()
}

/// Find the [`CodeActionDiff`] for a named action entry.
///
/// Returns `Some` if `name` matches one of the resolved action filenames.
/// The caller sets this as [`DiffCapable`] on the request for the diff middleware.
pub fn find_action_diff(resolved: &[ResolvedAction], name: &str, query: &LspQuery) -> Option<CodeActionDiff> {
    resolved
        .iter()
        .find(|e| e.file_name == name)
        .map(|entry| CodeActionDiff {
            query: query.clone(),
            action: entry.action.clone(),
            resolved: OnceLock::new(),
        })
}

/// Unified code action handler — implements [`DiffSource`] for both preview
/// and apply-on-delete.
///
/// Resolves the code action (via `codeAction/resolve` if needed), converts
/// the workspace edit to [`FileEditResult`]s via [`resolve_workspace_edit`].
pub struct CodeActionDiff {
    query: LspQuery,
    action: CodeAction,
    /// Cached result of `codeAction/resolve` to avoid duplicate LSP round-trips.
    resolved: OnceLock<Option<CodeAction>>,
}

impl Clone for CodeActionDiff {
    fn clone(&self) -> Self {
        Self {
            query: self.query.clone(),
            action: self.action.clone(),
            resolved: OnceLock::new(),
        }
    }
}
/// Methods for [`CodeActionDiff`].
impl CodeActionDiff {
    /// Resolve the code action to get a workspace edit.
    ///
    /// Returns the action with its edit populated, or `None` if resolution fails.
    /// Cached via `OnceLock` to avoid duplicate LSP round-trips when both
    /// `header_lines` and `compute_edits` need the resolved action.
    fn resolve_action(&self) -> Option<&CodeAction> {
        self.resolved
            .get_or_init(|| {
                if self.action.edit.is_some() {
                    return Some(self.action.clone());
                }
                self.query
                    .file_query()?
                    .resolve_code_action(self.action.clone())
                    .inspect_err(|err| tracing::debug!(?err, "code action resolution failed"))
                    .ok()
            })
            .as_ref()
    }
}

/// [`DiffSource`] implementation for [`CodeActionDiff`].
impl DiffSource for CodeActionDiff {
    /// Resolve and apply the code action to produce file edits.
    fn compute_edits(&self) -> Result<Vec<FileEditResult>> {
        let action = self.resolve_action().ok_or_else(|| eyre!(super::LSP_UNAVAILABLE))?;

        let workspace_edit = action
            .edit
            .as_ref()
            .ok_or_else(|| eyre!("code action `{}` has no workspace edit", self.action.title))?;

        resolve_workspace_edit(workspace_edit, self.query.path_resolver())
    }

    /// Return header lines describing the code action.
    fn header_lines(&self) -> Vec<String> {
        let kind = self.action.kind.as_ref().map_or("unknown", |k| k.as_str());

        let mut lines = vec![format!("Code action: {}", self.action.title), format!("Kind: {kind}")];

        // Check if this action can be resolved — if not, add a warning.
        if self.action.edit.is_none() && self.resolve_action().is_none() {
            lines.push(String::new());
            lines.push("No preview available — this action requires server-side execution.".to_owned());
        }

        lines
    }
}
