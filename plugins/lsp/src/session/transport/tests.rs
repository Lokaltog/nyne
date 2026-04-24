use std::io::Cursor;

use rstest::rstest;
use serde_json::json;

use super::*;

/// Wrap a JSON value in Content-Length framing for `read_message`.
fn frame(msg: &serde_json::Value) -> Vec<u8> {
    let payload = serde_json::to_string(msg).unwrap();
    format!("Content-Length: {}\r\n\r\n{payload}", payload.len()).into_bytes()
}
/// Parse `write_message` output into (declared_len, body), asserting the
/// Content-Length framing invariants along the way.
fn parse_written_message(buf: Vec<u8>) -> (usize, serde_json::Value) {
    let output = String::from_utf8(buf).unwrap();
    assert!(
        output.starts_with("Content-Length: "),
        "missing Content-Length prefix: {output:?}"
    );
    let (header, body) = output.split_once("\r\n\r\n").expect("missing \\r\\n\\r\\n boundary");
    let declared_len: usize = header
        .strip_prefix("Content-Length: ")
        .unwrap()
        .split("\r\n")
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(declared_len, body.len(), "Content-Length matches body size");
    (declared_len, serde_json::from_str(body).unwrap())
}

/// Write `msg` via `write_message` and return the parsed body (framing validated).
fn write_and_parse(msg: &serde_json::Value) -> serde_json::Value {
    let mut buf = Vec::new();
    write_message(&mut buf, msg).unwrap();
    parse_written_message(buf).1
}

/// Tests that `write_message` produces Content-Length framed JSON-RPC with
/// the full message body preserved (request form, with `id`).
#[rstest]
fn write_message_framing() {
    let msg = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {"rootUri": null},
    });
    let body = write_and_parse(&msg);
    assert_eq!(body["jsonrpc"], "2.0");
    assert_eq!(body["id"], 1);
    assert_eq!(body["method"], "initialize");
    assert_eq!(body["params"]["rootUri"], serde_json::Value::Null);
}

/// Tests that `write_message` faithfully writes a notification (no `id` field).
#[rstest]
fn write_message_notification_has_no_id() {
    let msg = json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {},
    });
    let body = write_and_parse(&msg);
    assert_eq!(body["jsonrpc"], "2.0");
    assert_eq!(body["method"], "initialized");
    assert!(body.get("id").is_none());
}

/// Tests that extra headers beyond Content-Length are ignored.
#[rstest]
fn read_headers_ignores_extra_headers() {
    let raw = "Content-Length: 42\r\nContent-Type: application/json\r\n\r\n";
    let mut cursor = Cursor::new(raw.as_bytes());
    let len = read_headers(&mut cursor).unwrap();
    assert_eq!(len, 42);
}

/// Tests that `read_message` correctly parses Content-Length framed JSON
/// payloads, both single messages and consecutive messages in a stream.
#[rstest]
#[case::single(vec![json!({"jsonrpc": "2.0", "id": 1, "result": {"ok": true}})])]
#[case::sequential(vec![
    json!({"jsonrpc": "2.0", "method": "progress", "params": {}}),
    json!({"jsonrpc": "2.0", "id": 1, "result": null}),
])]
fn read_message_parses(#[case] msgs: Vec<serde_json::Value>) {
    let mut data = Vec::new();
    for msg in &msgs {
        data.extend(frame(msg));
    }
    let mut cursor = Cursor::new(data);
    for msg in &msgs {
        assert_eq!(read_message(&mut cursor).unwrap(), *msg);
    }
}

/// Tests `parse_response_result` across result/error/missing-result scenarios.
/// `Ok(value)` and `Err { code, message_contains }` cover both branches;
/// code `-32601` additionally asserts `is_method_not_found`.
#[rstest]
#[case::extracts_result(
    json!({"jsonrpc": "2.0", "id": 1, "result": {"capabilities": {}}}),
    ExpectParse::Ok(json!({"capabilities": {}})),
)]
#[case::missing_result_is_null(
    json!({"jsonrpc": "2.0", "id": 1}),
    ExpectParse::Ok(serde_json::Value::Null),
)]
#[case::generic_error(
    json!({"jsonrpc": "2.0", "id": 1, "error": {"code": -32600, "message": "Invalid Request"}}),
    ExpectParse::Err { code: -32600, message_contains: "Invalid Request" },
)]
#[case::method_not_found(
    json!({"jsonrpc": "2.0", "id": 1, "error": {"code": -32601, "message": "Method not found"}}),
    ExpectParse::Err { code: -32601, message_contains: "Method not found" },
)]
fn parse_response_result_cases(#[case] msg: serde_json::Value, #[case] expect: ExpectParse) {
    let result = parse_response_result(&msg);
    match expect {
        ExpectParse::Ok(v) => assert_eq!(result.unwrap(), v),
        ExpectParse::Err { code, message_contains } => {
            let rpc_err = result
                .unwrap_err()
                .downcast::<JsonRpcError>()
                .expect("should be JsonRpcError");
            assert_eq!(rpc_err.code, code);
            assert!(rpc_err.message.contains(message_contains));
            assert_eq!(rpc_err.is_method_not_found(), code == -32601);
        }
    }
}

enum ExpectParse {
    Ok(serde_json::Value),
    Err { code: i64, message_contains: &'static str },
}

/// Verifies that `write_message` output can be read back by `read_message`.
#[rstest]
fn write_message_roundtrips_through_read_message() {
    let msg = json!({"jsonrpc": "2.0", "id": 42, "method": "test", "params": {}});
    let mut buf = Vec::new();
    write_message(&mut buf, &msg).unwrap();

    let mut cursor = Cursor::new(buf);
    let parsed = read_message(&mut cursor).unwrap();
    assert_eq!(parsed, msg);
}
