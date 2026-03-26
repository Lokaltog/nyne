//! Background threads that own the LSP server's stdio file descriptors.
//!
//! Two threads run per `LspClient`:
//! - **Writer** ([`writer_loop`]): drains a channel of outbound JSON-RPC messages
//!   and writes them to the server's stdin with Content-Length framing.
//! - **Reader** ([`reader_loop`]): reads all inbound messages from the server's
//!   stdout, dispatching responses to waiting callers via oneshot channels and
//!   storing push diagnostics in the [`DiagnosticStore`].
//!
//! This design decouples FUSE handler threads from raw stdio: they never touch
//! the fds directly, only interact via channels and the pending response map.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufWriter};
use std::sync::Arc;

use color_eyre::eyre::Result;
use lsp_types::PublishDiagnosticsParams;
use lsp_types::notification::{self as lsp_notif, Notification as _};
use parking_lot::Mutex;
use serde_json::json;
use tracing::{debug, trace, warn};

use super::io::TimeoutReader;
use super::transport;
use crate::lsp::diagnostic_store::DiagnosticStore;

/// Pending response map: JSON-RPC request id to a oneshot sender for the result.
///
/// Shared between `LspClient` (which inserts entries before sending a request)
/// and the reader thread (which removes entries when dispatching responses).
/// Protected by a `Mutex` because insertions and removals happen on different
/// threads. The lock is held only briefly -- never across I/O.
pub(super) type PendingResponses = Mutex<HashMap<i64, crossbeam_channel::Sender<Result<serde_json::Value>>>>;

/// Writer thread: reads JSON-RPC messages from the channel and writes
/// them to the server's stdin with Content-Length framing.
///
/// Exits when the channel disconnects (all senders dropped) or on
/// a write error (server process crashed / stdin closed).
pub(super) fn writer_loop(
    mut writer: BufWriter<File>,
    rx: &crossbeam_channel::Receiver<serde_json::Value>,
    server_name: &str,
) {
    while let Ok(msg) = rx.recv() {
        if let Err(e) = transport::write_message(&mut writer, &msg) {
            warn!(target: "nyne::lsp", server = %server_name, error = %e, "writer thread: write failed, exiting");
            return;
        }
    }
    debug!(target: "nyne::lsp", server = %server_name, "writer thread: channel closed, exiting");
}

/// Reader thread: reads JSON-RPC messages from the server's stdout and
/// dispatches them:
/// - **`publishDiagnostics` notifications**: parsed and stored in the
///   [`DiagnosticStore`], waking any FUSE threads waiting for fresh
///   diagnostics.
/// - **Other notifications** (no `id`): logged and discarded.
/// - **`workspace/diagnostic/refresh` requests**: acknowledged and used
///   to signal the [`DiagnosticStore`] that pull diagnostics are ready.
/// - **Other server requests** (`id` + `method`): acknowledged with an
///   empty success response via the write channel.
/// - **Responses** (`id`, no `method`): dispatched to the waiting caller
///   via the pending response map.
///
/// Exits on EOF (server exited) or persistent read errors. On exit,
/// drains the pending map so blocked callers get an error instead of
/// waiting until their deadline.
pub(super) fn reader_loop(
    mut reader: TimeoutReader,
    write_tx: &crossbeam_channel::Sender<serde_json::Value>,
    pending: &Arc<PendingResponses>,
    diagnostic_store: &Arc<DiagnosticStore>,
    server_name: &str,
) {
    loop {
        let msg = match transport::read_message(&mut reader) {
            Ok(msg) => msg,
            Err(e) => {
                // Per-read timeout: the server is quiet, not dead. Retry.
                if e.root_cause()
                    .downcast_ref::<io::Error>()
                    .is_some_and(|io_err| io_err.kind() == io::ErrorKind::TimedOut)
                {
                    continue;
                }
                // Real error or EOF — server is gone.
                debug!(target: "nyne::lsp", server = %server_name, error = %e, "reader thread exiting");
                drain_pending(pending, &format!("LSP server {server_name} disconnected: {e}"));
                return;
            }
        };

        // Notifications: no id field (or id: null per some implementations).
        let Some(id) = msg.get("id").filter(|v| !v.is_null()) else {
            handle_notification(&msg, diagnostic_store, server_name);
            continue;
        };

        // Server-initiated request: has both `id` and `method`.
        // Acknowledge with an empty success response via the write channel.
        if let Some(method) = msg.get("method").and_then(|m| m.as_str()) {
            debug!(
                target: "nyne::lsp",
                server = %server_name,
                server_id = %id,
                method,
                "acknowledging server request",
            );

            // workspace/diagnostic/refresh: the pull-model signal that
            // diagnostics are ready to be re-pulled. Clear dirty flags
            // so blocked get_or_wait calls unblock immediately.
            if method == "workspace/diagnostic/refresh" {
                diagnostic_store.signal_refresh();
            }

            let ack = json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": serde_json::Value::Null,
            });
            // If the write channel is closed, we're shutting down — just exit.
            if write_tx.send(ack).is_err() {
                drain_pending(pending, &format!("LSP server {server_name} shutting down"));
                return;
            }
            continue;
        }

        // Response: dispatch to the waiting caller.
        let response_id = id.as_i64().unwrap_or(-1);
        let result = transport::parse_response_result(&msg);

        let waiter = pending.lock().remove(&response_id);
        if let Some(tx) = waiter {
            // If the caller timed out and dropped the receiver, this
            // send fails silently — that's fine.
            tx.send(result).ok();
        } else {
            debug!(
                target: "nyne::lsp",
                server = %server_name,
                response_id,
                "received response for unknown/expired request id",
            );
        }
    }
}

/// Dispatch a server-initiated notification.
///
/// Only `textDocument/publishDiagnostics` is actionable -- its diagnostics
/// are parsed and stored in the [`DiagnosticStore`], which wakes any FUSE
/// threads blocked in [`DiagnosticStore::get_or_wait`]. All other
/// notifications are logged at `trace` level and discarded.
fn handle_notification(msg: &serde_json::Value, store: &DiagnosticStore, server_name: &str) {
    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("?");

    if method == lsp_notif::PublishDiagnostics::METHOD {
        let Some(params) = msg.get("params") else { return };
        match serde_json::from_value::<PublishDiagnosticsParams>(params.clone()) {
            Ok(pd) => {
                let Some(path) = url::Url::parse(pd.uri.as_str())
                    .ok()
                    .and_then(|u| u.to_file_path().ok())
                else {
                    return;
                };
                debug!(
                    target: "nyne::lsp",
                    server = %server_name,
                    path = %path.display(),
                    count = pd.diagnostics.len(),
                    "received publishDiagnostics",
                );
                store.publish(&path, pd.diagnostics);
            }
            Err(e) => {
                debug!(
                    target: "nyne::lsp",
                    server = %server_name,
                    error = %e,
                    "failed to parse publishDiagnostics params",
                );
            }
        }
        return;
    }

    trace!(
        target: "nyne::lsp",
        server = %server_name,
        method,
        "skipping server notification",
    );
}

/// Send errors to all pending callers and clear the map.
///
/// Called when the reader thread exits (server crashed or disconnected)
/// so that blocked `send_request` callers get an immediate error instead
/// of waiting until their deadline expires.
fn drain_pending(pending: &PendingResponses, reason: &str) {
    let mut map = pending.lock();
    if map.is_empty() {
        return;
    }
    let count = map.len();
    for (id, tx) in map.drain() {
        let err = color_eyre::eyre::eyre!("{reason} (pending request id={id})");
        tx.send(Err(err)).ok();
    }
    warn!(target: "nyne::lsp", count, reason, "drained pending responses");
}
