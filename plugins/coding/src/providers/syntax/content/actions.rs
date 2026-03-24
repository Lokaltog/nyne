// Code action content — unified DiffAction for preview and apply.

use std::ops::Range as StdRange;

use color_eyre::eyre::{Result, eyre};
use lsp_types::{CodeAction, Position, Range};
use nyne::dispatch::context::RequestContext;
use nyne::format;
use nyne::node::VirtualNode;

use crate::edit::diff_action::{DiffAction, DiffActionNode};
use crate::edit::plan::FileEditResult;
use crate::lsp::edit::resolve_workspace_edit;
use crate::lsp::handle::SymbolQuery;

/// Maximum length for the kebab-case slug derived from a code action title.
const ACTION_SLUG_MAX_LEN: usize = 60;

/// A resolved code action with its display index and generated filename.
pub(in crate::providers::syntax) struct ResolvedAction {
    pub action: CodeAction,
    pub file_name: String,
}

/// Query code actions for a symbol and return them as resolved entries.
///
/// Called at resolve time (readdir of `actions/`) to eagerly fetch
/// the list of available actions from the LSP server.
pub(in crate::providers::syntax) fn resolve_code_actions(
    query: &SymbolQuery,
    line_range: &StdRange<usize>,
) -> Vec<ResolvedAction> {
    let Some(fq) = query.file_query() else {
        return Vec::new();
    };

    let start = u32::try_from(line_range.start).unwrap_or(u32::MAX);
    let end = u32::try_from(line_range.end).unwrap_or(u32::MAX);
    let range = Range {
        start: Position {
            line: start,
            character: 0,
        },
        end: Position {
            line: end,
            character: u32::MAX,
        },
    };

    let Ok(actions) = fq.code_actions(range) else {
        return Vec::new();
    };

    actions
        .into_iter()
        .filter(|a| a.disabled.is_none())
        .enumerate()
        .map(|(i, action)| {
            let slug = format::to_kebab(&action.title, ACTION_SLUG_MAX_LEN);
            let file_name = format!("{:02}-{slug}.diff", i + 1);
            ResolvedAction { action, file_name }
        })
        .collect()
}

/// Build virtual file nodes for a list of resolved code actions.
///
/// Each action becomes a `.diff` file backed by [`DiffActionNode`] for both
/// preview (`Readable`) and apply-on-delete (`Unlinkable`).
pub(in crate::providers::syntax) fn build_action_nodes(
    resolved: Vec<ResolvedAction>,
    query: &SymbolQuery,
) -> Vec<VirtualNode> {
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
        let fq = self.query.file_query()?;
        fq.resolve_code_action(self.action.clone()).ok()
    }
}

/// [`DiffAction`] implementation for [`CodeActionDiff`].
impl DiffAction for CodeActionDiff {
    /// Resolve and apply the code action to produce file edits.
    fn compute_edits(&self, _ctx: &RequestContext<'_>) -> Result<Vec<FileEditResult>> {
        let action = self
            .resolve_action()
            .ok_or_else(|| eyre!(super::lsp::LSP_UNAVAILABLE))?;

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
