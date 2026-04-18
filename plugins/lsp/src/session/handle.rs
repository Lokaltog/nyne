// Provider-facing LSP handle — bridges LSP manager into cached LSP queries.
// Two levels:
//   `Handle`    — file-level: manager + real path + ext (shared across scopes)
//   `LspQuery`  — scoped: handle + pre-computed LSP range (per symbol/file)
//
// Created at resolve time, stored inside Readable impls, queried at read time.

//! Per-file LSP handles and scoped LSP queries.
//!
//! [`Handle`] is the resolve-time entry point: it acquires the appropriate
//! LSP client for a source file's extension and caches the overlay-rooted
//! path for downstream queries. [`LspQuery`] pairs the handle with a stored
//! LSP [`Range`](lsp_types::Range) — zero-width for point operations (hover,
//! references, rename), arbitrary-width for range operations (code actions,
//! inlay hints), or whole-file for file-wide operations.
//!
//! These handles are lightweight and clone-friendly — multiple node
//! readables for the same scope share cloned `LspQuery` instances.
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crop::Rope;
use lsp_types::{Position, Range};

use super::client::Client;
use super::manager::Manager;
use super::query::FileQuery;
use super::uri::byte_offset_to_position;

/// Per-file handle to an LSP language server.
///
/// Created by [`Self::for_file`] during provider resolve, which acquires
/// the appropriate [`Client`] based on file extension. The handle caches
/// the client reference and overlay-rooted file path so that downstream
/// queries (hover, references, rename, etc.) avoid repeated lookups.
///
/// Use [`at`](Self::at), [`over`](Self::over), or
/// [`whole_file`](Self::whole_file) to create a scoped [`LspQuery`] for
/// point-, range-, or file-scoped LSP operations.
pub struct Handle {
    manager: Arc<Manager>,
    /// Cached client — acquired at resolve time for capability checks.
    client: Arc<Client>,
    /// File path using the overlay root — matches the workspace root
    /// that LSP servers see (they run as daemon children on the overlay).
    lsp_file: PathBuf,
    /// File extension used for LSP language ID lookup.
    ext: String,
}

/// Per-file LSP handle for querying language server features.
impl Handle {
    /// Create a handle for the given source file, or `None` if no LSP server
    /// is available for this file's extension.
    ///
    /// Acquires (or spawns) the LSP client eagerly so that server capabilities
    /// are available at resolve time for feature gating. Also ensures the
    /// document is opened in the LSP server (`textDocument/didOpen`) —
    /// accessing a companion directory is the semantic equivalent of
    /// "opening" the file in an editor.
    pub(crate) fn for_file(lsp: &Arc<Manager>, source_file: &Path) -> Option<Arc<Self>> {
        let ext = source_file.extension().and_then(|e| e.to_str())?;
        let lsp = Arc::clone(lsp);

        let client = lsp.client_for_ext(ext)?;
        // LSP servers run as daemon children and see the backing
        // filesystem path, not the FUSE mount.
        let lsp_file = lsp.path_resolver().source_root().join(source_file);

        // Ensure the document is opened in the LSP server. This is
        // idempotent — only the first call per file sends the notification.
        lsp.ensure_document_open(&lsp_file, ext);

        Some(Arc::new(Self {
            manager: lsp,
            client,
            lsp_file,
            ext: ext.to_owned(),
        }))
    }

    /// Server capabilities — for checking feature support at resolve time.
    pub(crate) fn capabilities(&self) -> &lsp_types::ServerCapabilities { self.client.capabilities() }

    /// Create a zero-width [`LspQuery`] at the given byte offset.
    ///
    /// Converts the byte offset to an LSP `Position` via the supplied
    /// source, and stores it as a zero-width [`Range`] (`start == end`).
    /// Position-based LSP ops (hover, references, rename, etc.) read
    /// `range.start` via [`LspQuery::position`].
    ///
    /// Convenience shortcut for [`Self::over`]`(source, offset..offset)`.
    pub(crate) fn at(self: &Arc<Self>, source: &str, byte_offset: usize) -> LspQuery {
        self.over(source, byte_offset..byte_offset)
    }

    /// Create an [`LspQuery`] spanning a byte range.
    ///
    /// Converts the byte range to an LSP `Range` via the supplied source.
    /// Both range-based LSP ops (code actions, inlay hints, range
    /// formatting) and position-based ones (via [`LspQuery::position`],
    /// which reads `range.start`) route through this.
    pub(crate) fn over(self: &Arc<Self>, source: &str, byte_range: std::ops::Range<usize>) -> LspQuery {
        let rope = Rope::from(source);
        LspQuery {
            handle: Arc::clone(self),
            range: Range {
                start: byte_offset_to_position(&rope, byte_range.start),
                end: byte_offset_to_position(&rope, byte_range.end),
            },
        }
    }

    /// Create a whole-file [`LspQuery`].
    ///
    /// Builds a [`Range`] covering the entire source — used for file-wide
    /// LSP operations (file-level code actions, file-wide inlay hints).
    pub(crate) fn whole_file(self: &Arc<Self>, source: &str) -> LspQuery {
        self.over(source, 0..source.len())
    }

    /// Acquire a `FileQuery` for cached LSP operations.
    ///
    /// Returns `None` if the server has become unavailable since resolve time.
    pub(crate) fn file_query(&self) -> Option<FileQuery<'_>> { self.manager.file_query(&self.lsp_file, &self.ext) }

    /// The source-rooted file path used for LSP requests.
    pub(crate) fn lsp_file(&self) -> &Path { &self.lsp_file }

    /// The LSP client for this file's language server.
    pub(crate) fn client(&self) -> &Client { &self.client }

    /// Path resolver for rewriting LSP URIs from FUSE paths to source paths.
    pub(crate) fn path_resolver(&self) -> &super::path::PathResolver { self.manager.path_resolver() }
}

/// Scoped LSP query context — an [`Handle`] bound to an LSP [`Range`].
///
/// The range is the single source of truth for both position-based LSP
/// operations (hover, references, rename — use [`position()`](Self::position)
/// which reads `range.start`) and range-based ones (code actions, inlay
/// hints — use [`range()`](Self::range)). File-wide queries use the
/// whole-file range via [`Handle::whole_file`].
///
/// Construct via [`Handle::at`] (zero-width range at a byte offset),
/// [`Handle::over`] (arbitrary byte range), or [`Handle::whole_file`]
/// (entire file). Clone-friendly (`Arc<Handle>` inside) for embedding
/// in multiple node readables.
#[derive(Clone)]
pub struct LspQuery {
    handle: Arc<Handle>,
    /// LSP range (UTF-16 code units) this query covers. Zero-width for
    /// point queries (`start == end`), whole-file for file-wide queries.
    range: Range,
}

/// Scoped LSP query: accessors routed through a stored [`Range`].
impl LspQuery {
    /// LSP position for point-based operations — hover, definitions,
    /// references, rename, call/type hierarchy.
    ///
    /// Read from `range.start`: point queries are zero-width ranges.
    pub(crate) const fn position(&self) -> Position { self.range.start }

    /// LSP range for range-based operations — code actions, inlay hints,
    /// range formatting, range diagnostics.
    pub(crate) const fn range(&self) -> Range { self.range }
}

/// Auto-delegates `file_query`, `lsp_file`, `path_resolver`, etc. to the
/// inner [`Handle`] — avoids pure-forwarding boilerplate.
impl Deref for LspQuery {
    type Target = Handle;

    fn deref(&self) -> &Handle { &self.handle }
}
