// Scoped LSP query handle for a single file.
//
// Encapsulates client routing, path resolution, and cache key
// construction. Each method is a cached LSP query — SSOT for key
// format and caching discipline.

use std::path::Path;
use std::sync::Arc;

use color_eyre::eyre::Result;
use lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyOutgoingCall, CodeAction, Diagnostic, Hover, InlayHint, Location, Range,
};

use super::cache::CacheKey;
use super::client::{FilePosition, LspClient};
use super::manager::LspManager;

/// Scoped LSP query handle for a single file.
///
/// Each method constructs the appropriate `CacheKey` and delegates through
/// `LspManager::cached_query`, so callers never build keys manually.
pub struct FileQuery<'a> {
    manager: &'a LspManager,
    client: Arc<LspClient>,
    lsp_file: &'a Path,
}

/// Generate a positional `FileQuery` method: build `CacheKey`, delegate to
/// the named `LspClient` method, cache the result.
macro_rules! cached_pos_query {
    ($(#[doc = $doc:literal])* $name:ident => $client_method:ident, $cache_key:literal -> $ret:ty) => {
        $(#[doc = $doc])*
        pub(crate) fn $name(&self, line: u32, col: u32) -> Result<$ret> {
            let key = CacheKey { path: self.lsp_file, method: $cache_key, line, param: col };
            self.manager.cached_query(&key, || self.client.$client_method(&self.pos(line, col)))
        }
    };
}

/// Cached LSP queries scoped to a single file.
impl<'a> FileQuery<'a> {
    cached_pos_query! {
        /// Find all references to the symbol at the given position.
        references => references, "references" -> Vec<Location>
    }

    cached_pos_query! {
        /// Get hover documentation for the symbol at the given position.
        hover => hover, "hover" -> Option<Hover>
    }

    cached_pos_query! {
        /// Find implementations of the symbol at the given position.
        implementations => implementation, "implementation" -> Vec<Location>
    }

    cached_pos_query! {
        /// Get incoming calls to the symbol at the given position.
        incoming_calls => incoming_calls, "incomingCalls" -> Vec<CallHierarchyIncomingCall>
    }

    cached_pos_query! {
        /// Get outgoing calls from the symbol at the given position.
        outgoing_calls => outgoing_calls, "outgoingCalls" -> Vec<CallHierarchyOutgoingCall>
    }

    cached_pos_query! {
        /// Find the definition of the symbol at the given position.
        definition => definition, "definition" -> Vec<Location>
    }

    cached_pos_query! {
        /// Find the declaration of the symbol at the given position.
        declaration => declaration, "declaration" -> Vec<Location>
    }

    cached_pos_query! {
        /// Find the type definition of the symbol at the given position.
        type_definition => type_definition, "typeDefinition" -> Vec<Location>
    }

    /// Create a new file query handle for the given manager, client, and file.
    pub(crate) const fn new(manager: &'a LspManager, client: Arc<LspClient>, lsp_file: &'a Path) -> Self {
        Self {
            manager,
            client,
            lsp_file,
        }
    }

    /// Build a `FilePosition` at the given line and character in this file.
    const fn pos(&self, line: u32, character: u32) -> FilePosition<'a> {
        FilePosition {
            file: self.lsp_file,
            line,
            character,
        }
    }

    /// Get inlay hints for a line range within the file.
    ///
    /// Unlike positional queries, inlay hints are range-scoped — the cache
    /// key encodes both start and end lines (`line` = start, `col` = end)
    /// to distinguish different symbol ranges within the same file.
    pub(crate) fn inlay_hints(&self, range: Range) -> Result<Vec<InlayHint>> {
        let key = CacheKey {
            path: self.lsp_file,
            method: "inlayHint",
            line: range.start.line,
            param: range.end.line,
        };
        self.manager
            .cached_query(&key, || self.client.inlay_hints(self.lsp_file, range))
    }

    /// Get diagnostics for the file.
    ///
    /// Two strategies depending on server capabilities:
    ///
    /// - **Pull model** (server supports `textDocument/diagnostic`): sends
    ///   a pull request directly — the server responds synchronously with
    ///   current diagnostics. No freshness gate needed; the request itself
    ///   is the freshness mechanism.
    /// - **Push-only** (no pull capability): returns diagnostics from the
    ///   [`DiagnosticStore`], blocking if the file is dirty until the server
    ///   pushes fresh results or `diagnostics_timeout` expires.
    pub fn diagnostics(&self) -> Result<Vec<Diagnostic>> {
        if self.client.capabilities().diagnostic_provider.is_some() {
            // Pull model: pull directly. The server processes pending
            // didChange notifications before responding, so the result
            // reflects the current file content.
            self.client.diagnostics(self.lsp_file)
        } else {
            // Push-only: return whatever the store has, blocking if dirty.
            let timeout = self.manager.diagnostics_timeout();
            let store = self.client.diagnostic_store();
            Ok(store.get_or_wait(self.lsp_file, timeout))
        }
    }

    /// Preview a rename of the symbol at the given position.
    ///
    /// Returns the `WorkspaceEdit` that the LSP server would apply.
    /// Not cached — the result depends on the arbitrary `new_name` parameter,
    /// and this is a mutation preview, not a repeated read query.
    pub(crate) fn rename(&self, line: u32, col: u32, new_name: &str) -> Result<lsp_types::WorkspaceEdit> {
        self.client.rename(&self.pos(line, col), new_name)
    }

    /// Get code actions for a symbol's range.
    ///
    /// Not cached — code actions depend on current diagnostics and file state,
    /// so stale results would be misleading.
    pub(crate) fn code_actions(&self, range: Range) -> Result<Vec<CodeAction>> {
        let relevant: Vec<_> = self
            .diagnostics()
            .inspect_err(|e| tracing::debug!("failed to fetch diagnostics: {e}"))
            .unwrap_or_default()
            .into_iter()
            .filter(|d| ranges_overlap(&d.range, &range))
            .collect();
        self.client.code_actions(self.lsp_file, range, relevant)
    }

    /// Resolve a code action to fill in its workspace edit.
    pub(crate) fn resolve_code_action(&self, action: CodeAction) -> Result<CodeAction> {
        self.client.resolve_code_action(action)
    }
}

/// Whether two LSP ranges overlap (inclusive of touching boundaries).
///
/// Used by [`FileQuery::code_actions`] to filter code actions whose
/// reported range intersects the queried symbol span. Touching ranges
/// (e.g., end of one equals start of another) are considered overlapping
/// so that boundary-adjacent diagnostics are not missed.
fn ranges_overlap(a: &Range, b: &Range) -> bool { a.start <= b.end && b.start <= a.end }
