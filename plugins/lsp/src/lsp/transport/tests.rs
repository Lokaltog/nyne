use std::io::Cursor;

use serde_json::json;

use super::*;

/// Wrap a JSON value in Content-Length framing for `read_message`.
fn frame(msg: &serde_json::Value) -> Vec<u8> {
    let payload = serde_json::to_string(msg).unwrap();
    format!("Content-Length: {}\r\n\r\n{payload}", payload.len()).into_bytes()
}

/// Tests that `send_request` produces valid Content-Length framed JSON-RPC.
#[test]
fn send_request_framing() {
    let mut buf = Vec::new();
    send_request(&mut buf, 1, "initialize", json!({"rootUri": null})).unwrap();

    let output = String::from_utf8(buf).unwrap();
    assert!(output.starts_with("Content-Length: "));

    // Split header from body at the \r\n\r\n boundary.
    let parts: Vec<&str> = output.splitn(2, "\r\n\r\n").collect();
    assert_eq!(parts.len(), 2);

    let body: serde_json::Value = serde_json::from_str(parts[1]).unwrap();
    assert_eq!(body["jsonrpc"], "2.0");
    assert_eq!(body["id"], 1);
    assert_eq!(body["method"], "initialize");
    assert_eq!(body["params"]["rootUri"], serde_json::Value::Null);

    // Verify Content-Length matches body size.
    let declared_len: usize = parts[0].strip_prefix("Content-Length: ").unwrap().parse().unwrap();
    assert_eq!(declared_len, parts[1].len());
}

/// Tests that `send_notification` omits the `id` field.
#[test]
fn send_notification_has_no_id() {
    let mut buf = Vec::new();
    send_notification(&mut buf, "initialized", json!({})).unwrap();

    let output = String::from_utf8(buf).unwrap();
    let parts: Vec<&str> = output.splitn(2, "\r\n\r\n").collect();
    let body: serde_json::Value = serde_json::from_str(parts[1]).unwrap();

    assert_eq!(body["jsonrpc"], "2.0");
    assert_eq!(body["method"], "initialized");
    assert!(body.get("id").is_none());
}

/// Tests that extra headers beyond Content-Length are ignored.
#[test]
fn read_headers_ignores_extra_headers() {
    let raw = "Content-Length: 42\r\nContent-Type: application/json\r\n\r\n";
    let mut cursor = Cursor::new(raw.as_bytes());
    let len = read_headers(&mut cursor).unwrap();
    assert_eq!(len, 42);
}

/// Tests that `read_message` parses a Content-Length framed JSON value.
#[test]
fn read_message_parses_framed_json() {
    let msg = json!({"jsonrpc": "2.0", "id": 1, "result": {"ok": true}});
    let mut cursor = Cursor::new(frame(&msg));
    let parsed = read_message(&mut cursor).unwrap();
    assert_eq!(parsed, msg);
}

/// Tests that `read_message` reads two consecutive messages from a stream.
#[test]
fn read_message_reads_sequential_messages() {
    let msg1 = json!({"jsonrpc": "2.0", "method": "progress", "params": {}});
    let msg2 = json!({"jsonrpc": "2.0", "id": 1, "result": null});
    let mut data = frame(&msg1);
    data.extend(frame(&msg2));

    let mut cursor = Cursor::new(data);
    assert_eq!(read_message(&mut cursor).unwrap(), msg1);
    assert_eq!(read_message(&mut cursor).unwrap(), msg2);
}

/// Tests that `parse_response_result` extracts the `result` field.
#[test]
fn parse_response_result_extracts_result() {
    let msg = json!({"jsonrpc": "2.0", "id": 1, "result": {"capabilities": {}}});
    let result = parse_response_result(&msg).unwrap();
    assert_eq!(result, json!({"capabilities": {}}));
}

/// Tests that a missing `result` field returns `Null` rather than an error.
#[test]
fn parse_response_result_returns_null_when_result_missing() {
    let msg = json!({"jsonrpc": "2.0", "id": 1});
    let result = parse_response_result(&msg).unwrap();
    assert_eq!(result, serde_json::Value::Null);
}

/// Tests that an `error` field is returned as a `JsonRpcError`.
#[test]
fn parse_response_result_returns_json_rpc_error() {
    let msg = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "error": {"code": -32600, "message": "Invalid Request"},
    });
    let err = parse_response_result(&msg).unwrap_err();
    let rpc_err = err.downcast_ref::<JsonRpcError>().expect("should be JsonRpcError");
    assert_eq!(rpc_err.code, -32600);
    assert!(rpc_err.message.contains("Invalid Request"));
}

/// Tests that error code -32601 is recognized as method-not-found.
#[test]
fn parse_response_result_method_not_found() {
    let msg = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "error": {"code": -32601, "message": "Method not found"},
    });
    let err = parse_response_result(&msg).unwrap_err();
    let rpc_err = err.downcast_ref::<JsonRpcError>().expect("should be JsonRpcError");
    assert!(rpc_err.is_method_not_found());
}

/// Verifies that `write_message` output can be read back by `read_message`.
#[test]
fn write_message_roundtrips_through_read_message() {
    let msg = json!({"jsonrpc": "2.0", "id": 42, "method": "test", "params": {}});
    let mut buf = Vec::new();
    write_message(&mut buf, &msg).unwrap();

    let mut cursor = Cursor::new(buf);
    let parsed = read_message(&mut cursor).unwrap();
    assert_eq!(parsed, msg);
}
