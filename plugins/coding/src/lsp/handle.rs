// Provider-facing LSP handle — bridges ActivationContext into cached LSP queries.
//
// Two levels:
//   `LspHandle`    — file-level: manager + real path + ext (shared across symbols)
//   `SymbolQuery`  — symbol-level: handle + pre-computed LSP position (per fragment)
//
// Created at resolve time, stored inside Readable impls, queried at read time.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crop::Rope;
use lsp_types::Position;
use nyne::dispatch::activation::ActivationContext;
use nyne::types::vfs_path::VfsPath;

use super::client::LspClient;
use super::manager::LspManager;
use super::query::FileQuery;
use super::uri::byte_offset_to_position;
use crate::services::CodingServices;

/// Handle to LSP capabilities and queries for a single source file.
pub struct LspHandle {
    manager: Arc<LspManager>,
    /// Cached client — acquired at resolve time for capability checks.
    client: Arc<LspClient>,
    /// File path using the overlay root — matches the workspace root
    /// that LSP servers see (they run as daemon children on the overlay).
    lsp_file: PathBuf,
    ext: String,
}

/// Per-file LSP handle for querying language server features.
impl LspHandle {
    /// Create a handle for the given source file, or `None` if no LSP server
    /// is available for this file's extension.
    ///
    /// Acquires (or spawns) the LSP client eagerly so that server capabilities
    /// are available at resolve time for feature gating. Also ensures the
    /// document is opened in the LSP server (`textDocument/didOpen`) —
    /// accessing a companion directory is the semantic equivalent of
    /// "opening" the file in an editor.
    pub(crate) fn for_file(ctx: &ActivationContext, source_file: &VfsPath) -> Option<Arc<Self>> {
        let ext = source_file.extension()?;
        let services = CodingServices::get(ctx);
        let client = services.lsp.client_for_ext(ext)?;
        // Use overlay_root — LSP servers run as daemon children and see
        // the overlay merged path, not the FUSE mount.
        let lsp_file = ctx.overlay_root().join(source_file.as_str());

        // Ensure the document is opened in the LSP server. This is
        // idempotent — only the first call per file sends the notification.
        services.lsp.ensure_document_open(&lsp_file, ext);

        Some(Arc::new(Self {
            manager: Arc::clone(&services.lsp),
            client,
            lsp_file,
            ext: ext.to_owned(),
        }))
    }

    /// Server capabilities — for checking feature support at resolve time.
    pub(crate) fn capabilities(&self) -> &lsp_types::ServerCapabilities { self.client.capabilities() }

    /// Create a symbol-level query handle at the given byte offset.
    ///
    /// Converts the tree-sitter byte offset to an LSP `Position` using
    /// the provided source text.
    pub(crate) fn at(self: &Arc<Self>, source: &str, byte_offset: usize) -> SymbolQuery {
        let rope = Rope::from(source);
        let position = byte_offset_to_position(&rope, byte_offset);
        SymbolQuery {
            handle: Arc::clone(self),
            position,
        }
    }

    /// Acquire a `FileQuery` for cached LSP operations.
    ///
    /// Returns `None` if the server has become unavailable since resolve time.
    pub(crate) fn file_query(&self) -> Option<FileQuery<'_>> { self.manager.file_query(&self.lsp_file, &self.ext) }

    /// The overlay-rooted file path used for LSP requests.
    pub(crate) fn lsp_file(&self) -> &Path { &self.lsp_file }

    /// The LSP client for this file's language server.
    pub(crate) fn client(&self) -> &LspClient { &self.client }

    /// Path resolver for rewriting LSP URIs from FUSE paths to overlay paths.
    pub(crate) fn path_resolver(&self) -> &super::path::LspPathResolver { self.manager.path_resolver() }
}

/// Symbol-level LSP query context — an `LspHandle` bound to a specific position.
///
/// Clone-friendly for embedding in multiple `VirtualNode` readables
/// (e.g., REFERENCES.md and HOVER.md for the same symbol).
#[derive(Clone)]
/// Symbol-level LSP query context.
pub struct SymbolQuery {
    handle: Arc<LspHandle>,
    position: Position,
}

/// Per-symbol LSP query bound to a specific position.
impl SymbolQuery {
    /// Acquire a `FileQuery` for cached LSP operations.
    pub(crate) fn file_query(&self) -> Option<FileQuery<'_>> { self.handle.file_query() }

    /// The LSP position this query is bound to.
    pub(crate) const fn position(&self) -> Position { self.position }

    /// The overlay-rooted file path used for LSP requests.
    pub(crate) fn lsp_file(&self) -> &Path { self.handle.lsp_file() }

    /// Path resolver for rewriting LSP URIs from FUSE paths to overlay paths.
    pub(crate) fn path_resolver(&self) -> &super::path::LspPathResolver { self.handle.path_resolver() }
}
