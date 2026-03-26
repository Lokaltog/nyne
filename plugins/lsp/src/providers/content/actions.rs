//! LSP code action content — unified [`DiffAction`] for preview and apply.
//!
//! Each code action is exposed as a virtual diff file under `symbols/Foo@/actions/`.
//! Reading the file previews the edit; deleting it applies the action to the source.

use std::ops::Range as StdRange;

use color_eyre::eyre::{Result, eyre};
use lsp_types::CodeAction;
use nyne::dispatch::context::RequestContext;
use nyne::node::VirtualNode;
use nyne::text;
use nyne_source::edit::diff_action::{DiffAction, DiffActionNode};
use nyne_source::edit::plan::FileEditResult;

use crate::lsp::edit::resolve_workspace_edit;
use crate::lsp::handle::SymbolQuery;
use crate::lsp::uri::line_range_to_lsp_range;

/// Maximum length for the kebab-case slug derived from a code action title.
const ACTION_SLUG_MAX_LEN: usize = 60;

/// A resolved code action with its display index and generated filename.
pub(crate) struct ResolvedAction {
    pub action: CodeAction,
    pub file_name: String,
}

/// Query code actions for a symbol and return them as resolved entries.
///
/// Called at resolve time (readdir of `actions/`) to eagerly fetch
/// the list of available actions from the LSP server.
pub(crate) fn resolve_code_actions(query: &SymbolQuery, line_range: &StdRange<usize>) -> Vec<ResolvedAction> {
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

/// Build virtual file nodes for a list of resolved code actions.
///
/// Each action becomes a `.diff` file backed by [`DiffActionNode`] for both
/// preview (`Readable`) and apply-on-delete (`Unlinkable`).
pub(crate) fn build_action_nodes(resolved: Vec<ResolvedAction>, query: &SymbolQuery) -> Vec<VirtualNode> {
    resolved
        .into_iter()
        .map(|entry| {
            let action = CodeActionDiff {
                query: query.clone(),
                action: entry.action,
            };
            DiffActionNode::into_node(&entry.file_name, action)
        })
        .collect()
}

/// Unified code action handler — implements [`DiffAction`] for both preview
/// and apply-on-delete.
///
/// Resolves the code action (via `codeAction/resolve` if needed), converts
/// the workspace edit to [`FileEditResult`]s via [`resolve_workspace_edit`].
#[derive(Clone)]
struct CodeActionDiff {
    query: SymbolQuery,
    action: CodeAction,
}

/// Methods for [`CodeActionDiff`].
impl CodeActionDiff {
    /// Resolve the code action to get a workspace edit.
    ///
    /// Returns the action with its edit populated, or `None` if resolution fails.
    fn resolve_action(&self) -> Option<CodeAction> {
        if self.action.edit.is_some() {
            return Some(self.action.clone());
        }
        self.query
            .file_query()?
            .resolve_code_action(self.action.clone())
            .inspect_err(|err| tracing::debug!(?err, "code action resolution failed"))
            .ok()
    }
}

/// [`DiffAction`] implementation for [`CodeActionDiff`].
impl DiffAction for CodeActionDiff {
    /// Resolve and apply the code action to produce file edits.
    fn compute_edits(&self, _ctx: &RequestContext<'_>) -> Result<Vec<FileEditResult>> {
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
