//! LSP query methods on [`Client`](super::Client).
//!
//! Declarative macros eliminate boilerplate for method families that share
//! the same structure:
//! - [`require_capability!`] -- capability gate with early return and debug log
//! - [`goto_method!`] -- four goto methods (definition, declaration, type definition, implementation)
//! - [`hierarchy_query!`] -- two hierarchy methods (incoming/outgoing calls)
//!
//! Each generated method checks server capabilities, builds the appropriate
//! LSP params, sends the request, and normalizes the response to a `Vec`.

use std::path::Path;

use color_eyre::eyre::Result;
use lsp_types::notification::{self as lsp_notif, Notification as _};
use lsp_types::request::{self as lsp_req, Request as _};
use lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyIncomingCallsParams, CallHierarchyItem, CallHierarchyOutgoingCall,
    CallHierarchyOutgoingCallsParams, CallHierarchyPrepareParams, CodeAction, CodeActionContext, CodeActionOrCommand,
    CodeActionParams, CodeActionResponse, Diagnostic, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DocumentDiagnosticParams, DocumentDiagnosticReport, DocumentDiagnosticReportResult,
    GotoDefinitionResponse, Hover, InlayHint, InlayHintParams, Location, PartialResultParams, Range, ReferenceContext,
    ReferenceParams, RenameParams, SymbolInformation, TextDocumentContentChangeEvent, TextDocumentItem,
    WorkDoneProgressParams, WorkspaceEdit, WorkspaceSymbolParams,
};
use tracing::debug;

use super::{Client, FilePosition, uri};

/// Early-return `Ok(Default::default())` if the server lacks a capability.
///
/// Checks `self.capabilities.$cap` and returns an empty/default result when
/// `None`, logging a debug message. This avoids sending requests the server
/// would reject, and produces graceful degradation instead of errors.
macro_rules! require_capability {
    ($self:expr, $cap:ident, $feature:literal) => {
        if $self.capabilities.$cap.is_none() {
            debug!(
                target: "nyne::lsp",
                server = %$self.name,
                concat!("server does not support ", $feature, ", returning empty"),
            );
            return Ok(Default::default());
        }
    };
}

/// Generate a goto method: capability check → `TextDocumentPositionParams` → flatten response.
macro_rules! goto_method {
    ($(#[doc = $doc:literal])* $name:ident, $cap:ident, $feature:literal, $method:expr) => {
        $(#[doc = $doc])*
        pub(crate) fn $name(&self, pos: &FilePosition) -> Result<Vec<Location>> {
            require_capability!(self, $cap, $feature);
            self.goto_request($method, pos)
        }
    };
}

/// Generate a hierarchy query method: prepare item → build params → send → unwrap.
///
/// All hierarchy query params share the same shape: `{ item, work_done_progress_params, partial_result_params }`.
/// Method-not-found errors (`-32601`) are caught and return empty results,
/// so unsupported methods degrade gracefully.
macro_rules! hierarchy_query {
    ($(#[doc = $doc:literal])* $name:ident, $prepare:ident, $params:ident, $result:ty, $method:expr) => {
        $(#[doc = $doc])*
        pub(crate) fn $name(&self, pos: &FilePosition) -> Result<Vec<$result>> {
            let Some(item) = self.$prepare(pos)? else {
                return Ok(Vec::new());
            };
            let params = $params {
                item,
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            };
            let result: Option<Vec<$result>> = match self.send_request($method, params) {
                Ok(r) => r,
                Err(e) if e.downcast_ref::<super::transport::JsonRpcError>()
                    .is_some_and(|rpc| rpc.is_method_not_found()) =>
                {
                    debug!(
                        target: "nyne::lsp",
                        server = %self.name,
                        method = $method,
                        "server does not support method, returning empty",
                    );
                    return Ok(Vec::new());
                }
                Err(e) => return Err(e),
            };
            Ok(result.unwrap_or_default())
        }
    };
}

/// LSP query methods (goto, hover, diagnostics, rename, code actions).
impl Client {
    goto_method! {
        /// Find the definition of the symbol at the given position.
        definition, definition_provider, "definition", lsp_req::GotoDefinition::METHOD
    }

    goto_method! {
        /// Find the declaration of the symbol at the given position.
        declaration, declaration_provider, "declaration", lsp_req::GotoDeclaration::METHOD
    }

    goto_method! {
        /// Find the type definition of the symbol at the given position.
        type_definition, type_definition_provider, "type definition", lsp_req::GotoTypeDefinition::METHOD
    }

    goto_method! {
        /// Find implementations of the symbol at the given position.
        implementation, implementation_provider, "implementation", lsp_req::GotoImplementation::METHOD
    }

    hierarchy_query! {
        /// Get incoming calls to the symbol at the given position.
        incoming_calls, prepare_call_hierarchy,
        CallHierarchyIncomingCallsParams, CallHierarchyIncomingCall,
        lsp_req::CallHierarchyIncomingCalls::METHOD
    }

    hierarchy_query! {
        /// Get outgoing calls from the symbol at the given position.
        outgoing_calls, prepare_call_hierarchy,
        CallHierarchyOutgoingCallsParams, CallHierarchyOutgoingCall,
        lsp_req::CallHierarchyOutgoingCalls::METHOD
    }

    /// Notify the server that a document has been opened.
    ///
    /// Sends `textDocument/didOpen` with the full file content.
    /// Required before pull diagnostics will return results for
    /// servers that track document state (e.g., rust-analyzer).
    pub(crate) fn open_document(&self, file: &Path, language_id: &str, version: i32, content: String) -> Result<()> {
        let uri = uri::file_path_to_uri(file)?;
        debug!(
            target: "nyne::lsp",
            server = %self.name,
            ?uri,
            language_id,
            version,
            "sending textDocument/didOpen",
        );
        self.send_notification(lsp_notif::DidOpenTextDocument::METHOD, DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri,
                language_id: language_id.to_owned(),
                version,
                text: content,
            },
        })
    }

    /// Notify the server that a document's content has changed.
    ///
    /// Sends `textDocument/didChange` with full content sync
    /// (`TextDocumentSyncKind::Full`). The version must be strictly
    /// increasing for a given document URI.
    pub(crate) fn change_document(&self, file: &Path, version: i32, content: String) -> Result<()> {
        let text_document = uri::versioned_text_document_id(file, version)?;
        debug!(
            target: "nyne::lsp",
            server = %self.name,
            uri = ?text_document.uri,
            version,
            "sending textDocument/didChange",
        );
        self.send_notification(lsp_notif::DidChangeTextDocument::METHOD, DidChangeTextDocumentParams {
            text_document,
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: content,
            }],
        })
    }

    /// Notify the server that a document has been closed.
    ///
    /// Sends `textDocument/didClose` so the server can release resources
    /// associated with the document.
    pub(crate) fn close_document(&self, file: &Path) -> Result<()> {
        let text_document = uri::text_document_id(file)?;
        debug!(
            target: "nyne::lsp",
            server = %self.name,
            uri = ?text_document.uri,
            "sending textDocument/didClose",
        );
        self.send_notification(lsp_notif::DidCloseTextDocument::METHOD, DidCloseTextDocumentParams {
            text_document,
        })
    }

    /// Find all references to the symbol at the given position.
    pub(crate) fn references(&self, pos: &FilePosition) -> Result<Vec<Location>> {
        let params = ReferenceParams {
            text_document_position: pos.to_params()?,
            context: ReferenceContext {
                include_declaration: true,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let result: Option<Vec<Location>> = self.send_request(lsp_req::References::METHOD, params)?;
        Ok(result.unwrap_or_default())
    }

    /// Rename the symbol at the given position.
    pub(crate) fn rename(&self, pos: &FilePosition, new_name: &str) -> Result<WorkspaceEdit> {
        let params = RenameParams {
            text_document_position: pos.to_params()?,
            new_name: new_name.to_owned(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        self.send_request(lsp_req::Rename::METHOD, params)
    }

    /// Access the server's file operations capabilities, if advertised.
    fn file_operations(&self) -> Option<&lsp_types::WorkspaceFileOperationsServerCapabilities> {
        self.capabilities()
            .workspace
            .as_ref()
            .and_then(|ws| ws.file_operations.as_ref())
    }

    /// Request import-path updates before renaming files.
    ///
    /// Sends `workspace/willRenameFiles` to compute workspace edits (e.g.,
    /// import path rewrites) that should be applied before the actual file
    /// rename. Returns `None` if the server doesn't support file rename
    /// operations or returns a null edit.
    pub(crate) fn will_rename_files(&self, old_uri: &str, new_uri: &str) -> Result<Option<WorkspaceEdit>> {
        if self.file_operations().and_then(|fo| fo.will_rename.as_ref()).is_none() {
            debug!(
                target: "nyne::lsp",
                server = %self.name,
                "server does not support workspace/willRenameFiles, skipping",
            );
            return Ok(None);
        }

        self.send_request(lsp_req::WillRenameFiles::METHOD, rename_files_params(old_uri, new_uri))
    }

    /// Notify the server that a file rename has been completed.
    ///
    /// Fire-and-forget notification so the server can update its internal
    /// state (e.g., update its file index, re-resolve module paths).
    pub(crate) fn did_rename_files(&self, old_uri: &str, new_uri: &str) -> Result<()> {
        if self.file_operations().and_then(|fo| fo.did_rename.as_ref()).is_none() {
            debug!(
                target: "nyne::lsp",
                server = %self.name,
                "server does not support workspace/didRenameFiles, skipping",
            );
            return Ok(());
        }

        self.send_notification(lsp_notif::DidRenameFiles::METHOD, rename_files_params(old_uri, new_uri))
    }

    /// Get hover documentation for the symbol at the given position.
    pub(crate) fn hover(&self, pos: &FilePosition) -> Result<Option<Hover>> {
        require_capability!(self, hover_provider, "hover");
        self.send_request(lsp_req::HoverRequest::METHOD, pos.to_params()?)
    }

    /// Get inlay hints for the given file within the specified range.
    pub(crate) fn inlay_hints(&self, file: &Path, range: Range) -> Result<Vec<InlayHint>> {
        require_capability!(self, inlay_hint_provider, "inlay hints");

        let params = InlayHintParams {
            text_document: uri::text_document_id(file)?,
            range,
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        let result: Option<Vec<InlayHint>> = self.send_request(lsp_req::InlayHintRequest::METHOD, params)?;
        Ok(result.unwrap_or_default())
    }

    /// Pull diagnostics for a single file.
    ///
    /// Uses the `textDocument/diagnostic` pull model (LSP 3.17). Returns an
    /// empty vec if the server does not support diagnostic pull.
    pub(crate) fn diagnostics(&self, file: &Path) -> Result<Vec<Diagnostic>> {
        require_capability!(self, diagnostic_provider, "diagnostic pull");

        let params = DocumentDiagnosticParams {
            text_document: uri::text_document_id(file)?,
            identifier: None,
            previous_result_id: None,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let result: DocumentDiagnosticReportResult =
            match self.send_request(lsp_req::DocumentDiagnosticRequest::METHOD, params) {
                Ok(r) => r,
                Err(e) => {
                    debug!(
                        target: "nyne::lsp",
                        server = %self.name,
                        error = %e,
                        "textDocument/diagnostic request failed, returning empty",
                    );
                    return Ok(Vec::new());
                }
            };

        let items = match result {
            DocumentDiagnosticReportResult::Report(report) => match report {
                DocumentDiagnosticReport::Full(full) => {
                    let items = full.full_document_diagnostic_report.items;
                    debug!(
                        target: "nyne::lsp",
                        server = %self.name,
                        count = items.len(),
                        "diagnostics: full report",
                    );
                    items
                }
                DocumentDiagnosticReport::Unchanged(_) => {
                    debug!(target: "nyne::lsp", server = %self.name, "diagnostics: unchanged report");
                    Vec::new()
                }
            },
            DocumentDiagnosticReportResult::Partial(_) => {
                debug!(
                    target: "nyne::lsp",
                    server = %self.name,
                    "diagnostics: partial result (no items — likely serde(untagged) fallthrough)",
                );
                Vec::new()
            }
        };

        Ok(items)
    }

    /// Get code actions for a range, optionally scoped to diagnostics.
    pub(crate) fn code_actions(
        &self,
        file: &Path,
        range: Range,
        diagnostics: Vec<Diagnostic>,
    ) -> Result<Vec<CodeAction>> {
        require_capability!(self, code_action_provider, "code actions");

        let params = CodeActionParams {
            text_document: uri::text_document_id(file)?,
            range,
            context: CodeActionContext {
                diagnostics,
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let result: Option<CodeActionResponse> = self.send_request(lsp_req::CodeActionRequest::METHOD, params)?;
        Ok(result
            .unwrap_or_default()
            .into_iter()
            .filter_map(|item| match item {
                CodeActionOrCommand::CodeAction(action) => Some(action),
                CodeActionOrCommand::Command(_) => None,
            })
            .collect())
    }

    /// Resolve a code action to fill in its `edit` field.
    ///
    /// Servers that support `codeAction/resolve` return lightweight actions
    /// from `textDocument/codeAction` (without edits), deferring the
    /// expensive edit computation to this resolve step.
    pub(crate) fn resolve_code_action(&self, action: CodeAction) -> Result<CodeAction> {
        self.send_request(lsp_req::CodeActionResolveRequest::METHOD, action)
    }

    /// Unified goto: send `TextDocumentPositionParams`, flatten `GotoDefinitionResponse`.
    fn goto_request(&self, method: &str, pos: &FilePosition) -> Result<Vec<Location>> {
        let result: Option<GotoDefinitionResponse> = self.send_request(method, pos.to_params()?)?;
        Ok(goto_response_to_locations(result))
    }

    /// Prepare call hierarchy at position, returning the first item if any.
    fn prepare_call_hierarchy(&self, pos: &FilePosition) -> Result<Option<CallHierarchyItem>> {
        let params = CallHierarchyPrepareParams {
            text_document_position_params: pos.to_params()?,
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        let result: Option<Vec<CallHierarchyItem>> =
            self.send_request(lsp_req::CallHierarchyPrepare::METHOD, params)?;
        Ok(result.and_then(|v| v.into_iter().next()))
    }

    /// Search for workspace symbols matching a query string.
    ///
    /// Returns all symbols across the workspace whose name matches `query`.
    /// Results are in flat `SymbolInformation` format (no resolve step).
    pub(crate) fn workspace_symbol(&self, query: &str) -> Result<Vec<SymbolInformation>> {
        require_capability!(self, workspace_symbol_provider, "workspace/symbol");
        let params = WorkspaceSymbolParams {
            query: query.to_owned(),
            ..Default::default()
        };
        let result: Option<Vec<SymbolInformation>> =
            self.send_request(lsp_req::WorkspaceSymbolRequest::METHOD, params)?;
        Ok(result.unwrap_or_default())
    }
}

/// Build `RenameFilesParams` for a single file rename.
fn rename_files_params(old_uri: &str, new_uri: &str) -> lsp_types::RenameFilesParams {
    lsp_types::RenameFilesParams {
        files: vec![lsp_types::FileRename {
            old_uri: old_uri.to_owned(),
            new_uri: new_uri.to_owned(),
        }],
    }
}
/// Flatten a `GotoDefinitionResponse` into a plain `Vec<Location>`.
///
/// The LSP spec allows three response shapes (scalar, array, link array).
/// This normalizer lets all goto consumers work with a single `Vec<Location>`
/// type. `LocationLink` responses use `target_selection_range` (the precise
/// symbol range) rather than `target_range` (which may include surrounding
/// context).
fn goto_response_to_locations(response: Option<GotoDefinitionResponse>) -> Vec<Location> {
    match response {
        None => Vec::new(),
        Some(GotoDefinitionResponse::Scalar(loc)) => vec![loc],
        Some(GotoDefinitionResponse::Array(locs)) => locs,
        Some(GotoDefinitionResponse::Link(links)) => links
            .into_iter()
            .map(|link| Location {
                uri: link.target_uri,
                range: link.target_selection_range,
            })
            .collect(),
    }
}
