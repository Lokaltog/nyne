// LspClient — spawns a language server subprocess and exposes LSP operations.
//
// Split into:
//   mod.rs          — struct definition, lifecycle (spawn/init/shutdown/Drop), JSON-RPC plumbing
//   queries.rs      — all LSP query methods, with DRY helpers for common patterns
//   io.rs           — timeout-aware fd reading, stderr draining
//   threads.rs      — background reader/writer thread loops, pending response map
//   capabilities.rs — static client capabilities and propagated env vars

/// Server capability detection and feature checks.
mod capabilities;
/// Timeout-aware I/O for LSP server stdio.
mod io;
/// LSP request/response query helpers.
mod queries;
/// Background threads for reading LSP server output.
mod threads;

use std::collections::HashMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::thread::Builder;
use std::time::{Duration, Instant};
use std::{env, process};

use color_eyre::eyre::{Result, WrapErr, bail};
use lsp_types::notification::{Exit, Initialized, Notification as _};
use lsp_types::request::{Initialize, Request as _, Shutdown};
use lsp_types::{
    ClientInfo, InitializeParams, InitializeResult, InitializedParams, ServerCapabilities, TextDocumentPositionParams,
    Uri, WorkspaceFolder,
};
use nyne::process::Spawner;
use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use serde_json::json;
use tracing::{debug, info, trace, warn};

use self::capabilities::{PROPAGATED_ENV_VARS, client_capabilities};
use self::io::{TimeoutReader, drain_stderr};
use self::threads::{PendingResponses, reader_loop, writer_loop};
use super::diagnostic_store::DiagnosticStore;
use super::{transport, uri};

/// A file position for LSP queries: path + line + character.
///
/// Eliminates the repeated `(file: &Path, line: u32, character: u32)` triple
/// across all query methods. Provides conversion to `TextDocumentPositionParams`.
pub struct FilePosition<'a> {
    pub file: &'a Path,
    pub line: u32,
    pub character: u32,
}

/// Methods for converting file positions to LSP protocol types.
impl FilePosition<'_> {
    /// Convert to LSP `TextDocumentPositionParams` for requests.
    fn to_params(&self) -> Result<TextDocumentPositionParams> {
        Ok(TextDocumentPositionParams {
            text_document: uri::text_document_id(self.file)?,
            position: lsp_types::Position {
                line: self.line,
                character: self.character,
            },
        })
    }
}

/// Client for a single LSP server subprocess.
///
/// Communicates over stdin/stdout using JSON-RPC 2.0 with Content-Length
/// framing. Thread-safe: multiple FUSE threads can issue requests
/// concurrently without blocking each other.
///
/// Architecture: dedicated reader and writer threads own the stdio fds.
/// Callers interact exclusively through channels:
/// - **Writer thread**: receives serialized JSON-RPC messages via channel,
///   writes them with Content-Length framing.
/// - **Reader thread**: reads all incoming messages, dispatches responses
///   to waiting callers via oneshot channels, and acknowledges
///   server-initiated requests inline.
///
/// This eliminates Mutex contention on stdio from FUSE handler threads —
/// they only block on a bounded channel recv with a deadline.
pub struct LspClient {
    /// Server name (for logging and `LspManager` keying).
    name: String,
    /// Channel to the writer thread. Carries framed JSON-RPC messages.
    ///
    /// Unbounded: backpressure is unnecessary because the number of
    /// concurrent senders is bounded by the FUSE thread count (4).
    /// A bounded channel would risk latency spikes if the writer
    /// thread stalls briefly during a flush.
    write_tx: crossbeam_channel::Sender<serde_json::Value>,
    /// Pending request map shared with the reader thread.
    pending: Arc<PendingResponses>,
    next_id: AtomicI64,
    root_uri: Uri,
    /// Server capabilities discovered during initialize handshake.
    capabilities: ServerCapabilities,
    /// Overall timeout for a complete request-response cycle.
    /// Used as the deadline for `recv_deadline` on the response channel.
    response_timeout: Duration,
    /// Push diagnostics received via `textDocument/publishDiagnostics`.
    /// Shared with the reader thread which populates it.
    diagnostic_store: Arc<DiagnosticStore>,
}

/// Core LSP client lifecycle: spawning, initialization, and shutdown.
impl LspClient {
    /// Spawn a language server as a direct child of the daemon, perform the
    /// initialize handshake, and return a ready-to-use client.
    ///
    /// `name` is used for logging and as the key in `LspManager`.
    /// `root_dir` is the overlay merged path where the LSP server operates.
    ///
    /// Starts two background threads (reader + writer) that own the server's
    /// stdio fds. All subsequent communication goes through channels.
    pub(crate) fn spawn(
        server: &super::spec::LspServerDef,
        root_dir: &Path,
        spawner: &Spawner,
        response_timeout: Duration,
        extra_env: &HashMap<String, String>,
    ) -> Result<Self> {
        let name = server.name();
        let command = server.command_str();
        let args = server.args_slice();
        info!(
            target: "nyne::lsp",
            name,
            command,
            ?args,
            root = %root_dir.display(),
            ?response_timeout,
            "spawning language server",
        );

        // Build env vars to propagate. The spawner clears the environment
        // and sets only these — prevents shell hooks (direnv, conda, nvm)
        // from activating in the LSP server process.
        //
        // Sandbox config `env` entries are merged on top, allowing users to
        // inject or override specific variables.
        let mut env: Vec<(String, String)> = PROPAGATED_ENV_VARS
            .iter()
            .filter_map(|key| env::var(key).ok().map(|val| ((*key).to_owned(), val)))
            .collect();
        for (k, v) in extra_env {
            if let Some(entry) = env.iter_mut().find(|(ek, _)| ek == k) {
                entry.1.clone_from(v);
            } else {
                env.push((k.clone(), v.clone()));
            }
        }

        let fds = spawner
            .spawn(command, args, &env, root_dir)
            .wrap_err_with(|| format!("failed to spawn LSP server: {command}"))?;

        // Drain stderr on a background thread.
        {
            let stderr_file = File::from(fds.stderr);
            let server_name = name.to_owned();
            Builder::new()
                .name(format!("lsp-stderr-{name}"))
                .spawn(move || drain_stderr(stderr_file, &server_name))
                .ok();
        }

        let root_uri = uri::file_path_to_uri(root_dir)?;

        // Start writer thread — owns stdin.
        let (write_tx, write_rx) = crossbeam_channel::unbounded::<serde_json::Value>();
        let stdin_writer = BufWriter::new(File::from(fds.stdin));
        {
            let server_name = name.to_owned();
            Builder::new()
                .name(format!("lsp-writer-{name}"))
                .spawn(move || writer_loop(stdin_writer, &write_rx, &server_name))
                .wrap_err("failed to spawn LSP writer thread")?;
        }

        // Start reader thread — owns stdout.
        let pending: Arc<PendingResponses> = Arc::new(Mutex::new(HashMap::new()));
        let diagnostic_store = Arc::new(DiagnosticStore::new());
        let stdout_reader = TimeoutReader::from_owned_fd(fds.stdout, response_timeout);
        {
            let server_name = name.to_owned();
            let pending = Arc::clone(&pending);
            let diagnostic_store = Arc::clone(&diagnostic_store);
            let write_tx = write_tx.clone();
            Builder::new()
                .name(format!("lsp-reader-{name}"))
                .spawn(move || reader_loop(stdout_reader, &write_tx, &pending, &diagnostic_store, &server_name))
                .wrap_err("failed to spawn LSP reader thread")?;
        }

        let mut client = Self {
            name: name.to_owned(),
            write_tx,
            pending,
            next_id: AtomicI64::new(1),
            root_uri,
            capabilities: ServerCapabilities::default(),
            response_timeout,
            diagnostic_store,
        };

        client.initialize()?;
        Ok(client)
    }

    /// Server name (for logging and identification).
    pub(crate) fn name(&self) -> &str { &self.name }

    /// Server capabilities discovered during the initialize handshake.
    pub(crate) const fn capabilities(&self) -> &ServerCapabilities { &self.capabilities }

    /// Push diagnostics store shared with the reader thread.
    pub(crate) const fn diagnostic_store(&self) -> &Arc<DiagnosticStore> { &self.diagnostic_store }

    /// Send `initialize` request followed by `initialized` notification.
    fn initialize(&mut self) -> Result<()> {
        let workspace_folder = WorkspaceFolder {
            uri: self.root_uri.clone(),
            name: "root".to_owned(),
        };

        let params = InitializeParams {
            process_id: Some(process::id()),
            workspace_folders: Some(vec![workspace_folder]),
            capabilities: client_capabilities(),
            client_info: Some(ClientInfo {
                name: "nyne".to_owned(),
                version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            }),
            ..Default::default()
        };

        let result: InitializeResult = self
            .send_request(Initialize::METHOD, params)
            .wrap_err("LSP initialize handshake failed")?;

        self.capabilities = result.capabilities;

        info!(
            target: "nyne::lsp",
            server = %self.name,
            "LSP server initialized, sending initialized notification",
        );

        self.send_notification(Initialized::METHOD, InitializedParams {})?;
        Ok(())
    }

    /// Gracefully shut down the language server.
    ///
    /// Sends `shutdown` request, waits for the response, then sends `exit`
    /// notification. The `Spawner` reaps the child process on drop.
    fn shutdown(&self) -> Result<()> {
        info!(target: "nyne::lsp", server = %self.name, "shutting down LSP server");

        let _: Option<()> = self
            .send_request(Shutdown::METHOD, ())
            .wrap_err("LSP shutdown request failed")?;

        self.send_notification(Exit::METHOD, ())
            .wrap_err("LSP exit notification failed")?;

        info!(target: "nyne::lsp", server = %self.name, "LSP server shut down");
        Ok(())
    }

    /// Send a JSON-RPC request and deserialize the response.
    ///
    /// Constructs the request JSON, registers a oneshot response channel,
    /// sends via the writer thread, and waits with a deadline. No Mutex
    /// is held during the wait — multiple FUSE threads can have requests
    /// in flight concurrently.
    fn send_request<P: serde::Serialize, R: DeserializeOwned>(&self, method: &str, params: P) -> Result<R> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        debug!(target: "nyne::lsp", server = %self.name, method, id, "sending request");

        // Register the response channel before sending the request,
        // so the reader thread can dispatch even if the response arrives
        // before we call recv.
        let (resp_tx, resp_rx) = crossbeam_channel::bounded(1);
        self.pending.lock().insert(id, resp_tx);

        // Build and send the request via the writer thread.
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": serde_json::to_value(params)?,
        });
        if let Err(e) = self.write_tx.send(request) {
            self.pending.lock().remove(&id);
            bail!("LSP writer thread disconnected: {e}");
        }

        // Wait for the response with a deadline.
        let deadline = Instant::now() + self.response_timeout;
        let value = match resp_rx.recv_deadline(deadline) {
            Ok(result) => result?,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                self.pending.lock().remove(&id);
                warn!(target: "nyne::lsp", server = %self.name, method, id, "request timed out");
                bail!("LSP request timed out waiting for response (method={method}, id={id})");
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                self.pending.lock().remove(&id);
                bail!("LSP reader thread disconnected while waiting for response (method={method}, id={id})");
            }
        };

        debug!(
            target: "nyne::lsp",
            server = %self.name,
            method,
            id,
            is_null = value.is_null(),
            "received response",
        );

        trace!(target: "nyne::lsp::wire", server = %self.name, method, id, payload = %value, "raw response");

        serde_json::from_value(value).wrap_err_with(|| format!("failed to deserialize {method} response"))
    }

    /// Send a JSON-RPC notification (fire-and-forget).
    ///
    /// Serializes the notification and sends it via the writer thread.
    /// Returns immediately — no response is expected.
    fn send_notification<P: serde::Serialize>(&self, method: &str, params: P) -> Result<()> {
        debug!(target: "nyne::lsp", server = %self.name, method, "sending notification");

        let notification = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": serde_json::to_value(params)?,
        });
        self.write_tx
            .send(notification)
            .map_err(|e| color_eyre::eyre::eyre!("LSP writer thread disconnected: {e}"))?;

        Ok(())
    }
}

/// Gracefully shuts down the language server on drop.
impl Drop for LspClient {
    /// Cleans up resources.
    fn drop(&mut self) {
        if let Err(e) = self.shutdown() {
            warn!(target: "nyne::lsp", server = %self.name, error = %e, "LSP shutdown failed");
        }
    }
}
