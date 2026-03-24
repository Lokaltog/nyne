use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use color_eyre::eyre::Result;
use lsp_types::SymbolInformation;
use nyne::process::Spawner;
use parking_lot::{Mutex, RwLock};
use tracing::{debug, warn};

use super::LspRegistry;
use super::cache::LspCache;
use super::client::LspClient;
use crate::config::lsp::LspConfig;
use crate::syntax::SyntaxRegistry;

/// Tracked state for a document opened via `textDocument/didOpen`.
struct OpenDocument {
    /// File extension — used to look up the LSP client without re-deriving from the path.
    ext: String,
    /// Monotonically increasing version, sent with `didOpen` and each subsequent
    /// `didChange`. The LSP spec requires strictly increasing versions per document URI.
    version: i32,
}

/// Version tracking and state for an LSP-opened document.
impl OpenDocument {
    /// Initial version for a newly opened document. This is the single source
    /// of truth — `LspClient::open_document` receives this value.
    const INITIAL_VERSION: i32 = 0;

    /// Create a new document tracker with initial version.
    const fn new(ext: String) -> Self {
        Self {
            ext,
            version: Self::INITIAL_VERSION,
        }
    }
}

/// Manages LSP server lifecycle and cached queries.
pub struct LspManager {
    /// LSP server registry (built-in defaults + config overrides).
    registry: LspRegistry,
    /// Syntax registry — for gating LSP on tree-sitter support.
    syntax: Arc<SyntaxRegistry>,
    /// Global LSP settings from config.
    config: LspConfig,
    /// Spawner for launching LSP servers as direct daemon children.
    spawner: Arc<Spawner>,
    /// Extra environment variables from sandbox config, merged into LSP
    /// server processes on top of the default propagated set.
    sandbox_env: HashMap<String, String>,
    /// Active clients, keyed by server name.
    clients: RwLock<HashMap<String, Arc<LspClient>>>,
    /// Documents opened via `textDocument/didOpen`, tracked for lifecycle
    /// management. Maps overlay path → `(extension, version)`.
    ///
    /// The extension is stored so the client can be looked up for
    /// `didChange`/`didClose` without re-deriving from the path.
    /// The version is incremented on each `didChange` notification
    /// and used for the `VersionedTextDocumentIdentifier`.
    open_documents: Mutex<HashMap<PathBuf, OpenDocument>>,
    /// TTL-based result cache.
    cache: LspCache,
    /// Path resolver for LSP URIs (`fuse_root` → `overlay_root` rewriting).
    path_resolver: super::path::LspPathResolver,
}

/// Manages LSP client lifecycle, document tracking, and query routing.
impl LspManager {
    /// Create a new manager with the given registry, config, and process spawner.
    pub(crate) fn new(
        registry: LspRegistry,
        syntax: Arc<SyntaxRegistry>,
        config: LspConfig,
        spawner: Arc<Spawner>,
        sandbox_env: HashMap<String, String>,
        path_resolver: super::path::LspPathResolver,
    ) -> Self {
        let cache = LspCache::new(config.cache_ttl);
        Self {
            registry,
            syntax,
            config,
            spawner,
            sandbox_env,
            clients: RwLock::new(HashMap::new()),
            open_documents: Mutex::new(HashMap::new()),
            cache,
            path_resolver,
        }
    }

    /// Whether LSP is enabled at all (config toggle).
    pub(crate) const fn is_enabled(&self) -> bool { self.config.enabled }

    /// Return the unique set of server command names across all extensions.
    ///
    /// Delegates to the underlying `LspRegistry` -- avoids rebuilding the
    /// registry when only the command list is needed.
    pub(crate) fn server_commands(&self) -> impl Iterator<Item = &str> { self.registry.server_commands() }

    /// Whether an extension has both LSP and syntax (tree-sitter) support.
    ///
    /// LSP is gated on syntax — if there's no tree-sitter grammar for
    /// an extension, LSP queries are not available for it.
    fn has_lsp_support(&self, ext: &str) -> bool {
        self.is_enabled() && self.syntax.get(ext).is_some() && !self.registry.servers_for(ext).is_empty()
    }

    /// Get or spawn the primary LSP client for a file extension.
    ///
    /// Returns `None` if:
    /// - LSP is disabled in config
    /// - No syntax (tree-sitter) is registered for this extension
    /// - No LSP server is registered for this extension
    /// - Detection fails (project markers not found)
    /// - Server fails to spawn
    pub(crate) fn client_for_ext(&self, ext: &str) -> Option<Arc<LspClient>> {
        if !self.has_lsp_support(ext) {
            return None;
        }
        // Return the first server that is applicable and spawnable.
        for server_def in self.registry.servers_for(ext) {
            if !server_def.is_applicable(self.path_resolver.overlay_root()) {
                continue;
            }
            if let Some(client) = self.get_or_spawn(server_def) {
                return Some(client);
            }
        }
        None
    }

    /// Get all active LSP clients for a file extension.
    ///
    /// Unlike `client_for_ext` which returns only the primary, this spawns
    /// and returns all applicable servers. Used when we want to query
    /// multiple servers (e.g., diagnostics from both a type checker and a linter).
    pub(crate) fn all_clients_for_ext(&self, ext: &str) -> Vec<Arc<LspClient>> {
        if !self.has_lsp_support(ext) {
            return Vec::new();
        }
        self.registry
            .servers_for(ext)
            .iter()
            .filter(|def| def.is_applicable(self.path_resolver.overlay_root()))
            .filter_map(|def| self.get_or_spawn(def))
            .collect()
    }

    /// Execute a cached LSP query. If the result is cached and fresh,
    /// returns it without querying the server.
    pub(crate) fn cached_query<T, F>(&self, key: &super::cache::CacheKey, query: F) -> Result<T>
    where
        T: Clone + Send + Sync + 'static,
        F: FnOnce() -> Result<T>,
    {
        if let Some(cached) = self.cache.get::<T>(key) {
            return Ok(cached);
        }
        let result = query()?;
        self.cache.insert(key, result.clone());
        Ok(result)
    }

    /// Access the underlying cache directly.
    pub(crate) const fn cache(&self) -> &LspCache { &self.cache }

    /// Get the diagnostics timeout from config.
    pub(crate) const fn diagnostics_timeout(&self) -> Duration { self.config.diagnostics_timeout }

    /// Path resolver for rewriting LSP URIs from FUSE paths to overlay paths.
    pub(crate) const fn path_resolver(&self) -> &super::path::LspPathResolver { &self.path_resolver }

    /// Ensure a document is opened in the LSP server via `textDocument/didOpen`.
    ///
    /// Sends the notification on first call for a given file; subsequent calls
    /// are no-ops until the document is explicitly closed via [`Self::close_document`].
    /// Reads the file content from the overlay path.
    pub(crate) fn ensure_document_open(&self, lsp_file: &Path, ext: &str) {
        // Fast check: already open?
        {
            let docs = self.open_documents.lock();
            if docs.contains_key(lsp_file) {
                return;
            }
        }

        let Some(language_id) = self.registry.language_id_for(ext) else {
            return;
        };
        let Some(client) = self.client_for_ext(ext) else {
            return;
        };

        // Read file content from overlay (not FUSE — avoids re-entrancy).
        let content = match fs::read_to_string(lsp_file) {
            Ok(c) => c,
            Err(e) => {
                debug!(
                    target: "nyne::lsp",
                    path = %lsp_file.display(),
                    error = %e,
                    "failed to read file for didOpen, skipping",
                );
                return;
            }
        };

        let doc = OpenDocument::new(ext.to_owned());
        if let Err(e) = client.open_document(lsp_file, language_id, doc.version, content) {
            debug!(
                target: "nyne::lsp",
                path = %lsp_file.display(),
                error = %e,
                "textDocument/didOpen failed",
            );
            return;
        }

        let mut docs = self.open_documents.lock();
        docs.insert(lsp_file.to_path_buf(), doc);
    }

    /// Notify the LSP server that a file's content has changed and
    /// invalidate cached results.
    ///
    /// If the document was previously opened via `didOpen`, sends
    /// `textDocument/didChange` with the new content (full sync) and
    /// increments the tracked version. The document stays open — no
    /// `didClose`/`didOpen` round-trip needed.
    ///
    /// If the content cannot be read (file deleted), falls back to
    /// closing the document so the next access triggers a fresh `didOpen`.
    ///
    /// If the document was not open, this is a cache-only invalidation
    /// (the next access will trigger `didOpen` with fresh content).
    ///
    /// Called from the watcher invalidation path when the real file changes.
    pub(crate) fn invalidate_file(&self, path: &Path) {
        let change = {
            let mut docs = self.open_documents.lock();
            docs.get_mut(path).map(|doc| {
                doc.version += 1;
                (doc.ext.clone(), doc.version)
            })
        };

        if let Some((ext, version)) = change
            && let Some(client) = self.client_for_ext(&ext)
        {
            if Self::send_did_change(&client, path, version) {
                // Mark diagnostics dirty so the next read blocks until the
                // server pushes fresh `publishDiagnostics`.
                client.diagnostic_store().mark_dirty(path);
            } else {
                // Content unreadable (file deleted?) — close the document so
                // the next access triggers a fresh didOpen if the file reappears.
                self.close_document(path);
            }
        }

        self.cache.invalidate_file(path);
    }

    /// Close a document in the LSP server and remove it from tracking.
    ///
    /// Sends `textDocument/didClose` so the server can release resources.
    /// After this, the next access will re-open the document via `didOpen`.
    pub(crate) fn close_document(&self, path: &Path) {
        let doc = {
            let mut docs = self.open_documents.lock();
            docs.remove(path)
        };

        if let Some(doc) = doc
            && let Some(client) = self.client_for_ext(&doc.ext)
        {
            client.diagnostic_store().remove(path);
            if let Err(e) = client.close_document(path) {
                debug!(
                    target: "nyne::lsp",
                    path = %path.display(),
                    error = %e,
                    "textDocument/didClose failed",
                );
            }
        }
    }

    /// Compute and apply import-path updates before a file rename.
    ///
    /// Sends `workspace/willRenameFiles` to the appropriate LSP server
    /// and applies the returned `WorkspaceEdit` (e.g., import path
    /// rewrites). Must be called **before** the actual file rename.
    ///
    /// No-op if no LSP server supports file rename operations for this
    /// file type. LSP errors are logged but do not propagate — the caller
    /// is responsible for performing the actual file rename regardless.
    pub(crate) fn will_rename_file(&self, old_path: &Path, new_path: &Path) {
        let Some((client, old_uri, new_uri)) = self.resolve_rename_uris(old_path, new_path) else {
            return;
        };

        match client.will_rename_files(&old_uri, &new_uri) {
            Ok(Some(edit)) =>
                if let Err(e) = super::edit::apply_workspace_edit(&edit, &self.path_resolver) {
                    warn!(
                        target: "nyne::lsp",
                        old = %old_path.display(),
                        new = %new_path.display(),
                        error = %e,
                        "failed to apply willRenameFiles workspace edit",
                    );
                },
            Ok(None) => {}
            Err(e) => {
                warn!(
                    target: "nyne::lsp",
                    old = %old_path.display(),
                    new = %new_path.display(),
                    error = %e,
                    "workspace/willRenameFiles request failed",
                );
            }
        }
    }

    /// Notify the LSP server that a file rename has completed.
    ///
    /// Sends `workspace/didRenameFiles` and invalidates cached LSP
    /// results. Must be called **after** the actual file rename.
    pub(crate) fn did_rename_file(&self, old_path: &Path, new_path: &Path) {
        let Some((client, old_uri, new_uri)) = self.resolve_rename_uris(old_path, new_path) else {
            return;
        };

        if let Err(e) = client.did_rename_files(&old_uri, &new_uri) {
            warn!(
                target: "nyne::lsp",
                old = %old_path.display(),
                new = %new_path.display(),
                error = %e,
                "workspace/didRenameFiles notification failed",
            );
        }

        self.cache.invalidate_file(old_path);
        self.cache.invalidate_file(new_path);
    }

    /// Resolve LSP client and file URIs for a rename operation.
    ///
    /// Returns `None` if no LSP client is available for the file's
    /// extension or if the paths cannot be converted to file URIs.
    fn resolve_rename_uris(&self, old_path: &Path, new_path: &Path) -> Option<(Arc<LspClient>, String, String)> {
        let ext = old_path.extension().and_then(|e| e.to_str()).unwrap_or_default();
        let client = self.client_for_ext(ext)?;

        let old_uri = super::uri::file_path_to_uri_string(old_path).ok()?;
        let new_uri = super::uri::file_path_to_uri_string(new_path).ok()?;

        if old_uri == new_uri {
            warn!(
                target: "nyne::lsp",
                path = %old_path.display(),
                "rename paths resolve to the same URI, skipping",
            );
            return None;
        }

        Some((client, old_uri, new_uri))
    }

    /// Read updated content from overlay and send `textDocument/didChange`.
    ///
    /// Returns `true` if the notification was sent successfully, `false` if
    /// the file could not be read (e.g., deleted) or the notification failed.
    fn send_did_change(client: &LspClient, path: &Path, version: i32) -> bool {
        // Read from overlay (not FUSE — avoids re-entrancy).
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                debug!(
                    target: "nyne::lsp",
                    path = %path.display(),
                    error = %e,
                    "failed to read file for didChange",
                );
                return false;
            }
        };

        if let Err(e) = client.change_document(path, version, content) {
            debug!(
                target: "nyne::lsp",
                path = %path.display(),
                error = %e,
                "textDocument/didChange failed",
            );
            return false;
        }

        true
    }

    /// Create a scoped query handle for a file, or `None` if no LSP server
    /// is available for this extension.
    ///
    /// The returned `FileQuery` encapsulates client routing and cache key
    /// construction — callers never build `CacheKey`s manually.
    pub(crate) fn file_query<'a>(&'a self, lsp_file: &'a Path, ext: &str) -> Option<super::query::FileQuery<'a>> {
        let client = self.client_for_ext(ext)?;
        Some(super::query::FileQuery::new(self, client, lsp_file))
    }

    /// Summary of active LSP clients (for status reporting).
    pub(crate) fn status(&self) -> Vec<(String, bool)> {
        self.clients.read().keys().map(|name| (name.clone(), true)).collect()
    }

    /// Query all active LSP clients for workspace symbols matching a query.
    ///
    /// Queries already-running servers only — does not spawn on demand.
    /// Servers are started eagerly at activation via [`Self::spawn_all_applicable`],
    /// so they are typically available by the time this is called.
    /// Results are truncated to the configured `workspace_symbol_limit`.
    pub(crate) fn workspace_symbols(&self, query: &str) -> Vec<SymbolInformation> {
        let mut results: Vec<SymbolInformation> = self
            .clients
            .read()
            .values()
            .flat_map(|client| client.workspace_symbol(query).unwrap_or_default())
            .collect();
        results.truncate(self.config.workspace_symbol_limit);
        results
    }

    /// Eagerly spawn LSP servers for all registered extensions.
    ///
    /// Iterates every extension in the registry and triggers the normal
    /// spawn-if-applicable path. Idempotent — already-running servers are
    /// skipped via the fast path in `get_or_spawn`. Intended to be called
    /// from a background thread during activation so servers are warm by
    /// the time workspace-wide queries arrive.
    pub(crate) fn spawn_all_applicable(&self) {
        for ext in self.registry.extensions() {
            self.client_for_ext(ext);
        }
    }

    /// Return an existing client for the given server definition, or spawn one.
    fn get_or_spawn(&self, def: &super::spec::LspServerDef) -> Option<Arc<LspClient>> {
        // Fast path: already running.
        if let Some(client) = self.clients.read().get(def.name()) {
            return Some(Arc::clone(client));
        }

        // Slow path: spawn as a direct daemon child. LSP servers use
        // the overlay path (not FUSE) to avoid re-entrancy deadlocks.
        match LspClient::spawn(
            def,
            self.path_resolver.overlay_root(),
            &self.spawner,
            self.config.response_timeout,
            &self.sandbox_env,
        ) {
            Ok(client) => {
                let arc = Arc::new(client);
                self.clients.write().insert(def.name().to_owned(), Arc::clone(&arc));
                Some(arc)
            }
            Err(e) => {
                warn!(
                    target: "nyne::lsp",
                    server = def.name(),
                    error = %e,
                    "failed to spawn LSP server, skipping",
                );
                None
            }
        }
    }
}

/// Unit tests.
#[cfg(test)]
mod tests;
