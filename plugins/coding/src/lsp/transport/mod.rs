// JSON-RPC 2.0 framing over stdio with Content-Length headers.
//
// The LSP protocol is bidirectional: servers can send requests to the client
// (e.g., `window/workDoneProgress/create`, `client/registerCapability`).
// `read_response` handles these by sending back empty success responses,
// preventing deadlocks where the server blocks waiting for an acknowledgement.

use std::io::{BufRead, Write};

use color_eyre::eyre::{Result, eyre};
use serde::Serialize;
use serde_json::json;
use tracing::trace;

/// JSON-RPC error response from the language server.
///
/// Structured so callers can match on `code` (e.g., `-32601` = method not found)
/// without parsing error message strings.
#[derive(Debug, thiserror::Error)]
#[error("JSON-RPC error {code}: {message}")]
/// JSON-RPC error response from the language server.
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
}

impl JsonRpcError {
    /// JSON-RPC standard error code for "method not found".
    pub const METHOD_NOT_FOUND: i64 = -32601;

    pub const fn is_method_not_found(&self) -> bool { self.code == Self::METHOD_NOT_FOUND }
}

/// Send a JSON-RPC request with Content-Length header.
pub fn send_request(writer: &mut impl Write, id: i64, method: &str, params: impl Serialize) -> Result<()> {
    let body = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    write_message(writer, &body)
}

/// Send a JSON-RPC notification (no id, no response expected).
pub fn send_notification(writer: &mut impl Write, method: &str, params: impl Serialize) -> Result<()> {
    let body = json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    });
    write_message(writer, &body)
}

/// Extract the result from a JSON-RPC response, or return a `JsonRpcError`.
///
/// Shared between `read_response` (synchronous loop) and the async reader
/// thread's dispatch logic. Single source of truth for response parsing.
pub fn parse_response_result(msg: &serde_json::Value) -> Result<serde_json::Value> {
    if let Some(error) = msg.get("error") {
        let message = error
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown error");
        let code = error.get("code").and_then(serde_json::Value::as_i64).unwrap_or(0);
        return Err(JsonRpcError {
            code,
            message: message.to_owned(),
        }
        .into());
    }
    Ok(msg.get("result").cloned().unwrap_or(serde_json::Value::Null))
}

/// Read a single JSON-RPC message from the stream.
pub fn read_message(reader: &mut impl BufRead) -> Result<serde_json::Value> {
    let content_length = read_headers(reader)?;
    let mut buf = vec![0u8; content_length];
    reader.read_exact(&mut buf)?;

    let msg: serde_json::Value = serde_json::from_slice(&buf)?;
    trace!(target: "nyne::lsp", raw = %msg, "received JSON-RPC message");
    Ok(msg)
}

/// Serialize a JSON value and write it with Content-Length framing.
pub fn write_message(writer: &mut impl Write, body: &serde_json::Value) -> Result<()> {
    let payload = serde_json::to_string(body)?;
    trace!(target: "nyne::lsp", %payload, "sending JSON-RPC message");
    write!(writer, "Content-Length: {}\r\n\r\n{payload}", payload.len())?;
    writer.flush()?;
    Ok(())
}

/// Parse Content-Length from headers. Returns the content length.
fn read_headers(reader: &mut impl BufRead) -> Result<usize> {
    let mut content_length: Option<usize> = None;
    let mut line = String::new();

    loop {
        line.clear();
        reader.read_line(&mut line)?;

        // Empty line (just \r\n) marks end of headers.
        if line == "\r\n" {
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }

        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = Some(value.trim().parse().map_err(|e| eyre!("invalid Content-Length: {e}"))?);
        }
    }

    content_length.ok_or_else(|| eyre!("missing Content-Length header"))
}

#[cfg(test)]
mod tests;
