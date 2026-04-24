use rstest::rstest;

use super::*;
use crate::plugin::control::{ControlCommand, ProcessTable};
use crate::test_support::stub_activation_context;

fn run_dispatch(commands: Vec<ControlCommand>, name: &str, payload: serde_json::Value) -> Option<serde_json::Value> {
    let registry = ControlRegistry::from_commands(commands);
    let ctx = stub_activation_context();
    let procs: ProcessTable = Default::default();
    let ctrl_ctx = ControlContext {
        activation: &ctx,
        processes: &procs,
    };
    registry.dispatch(name, payload, &ctrl_ctx)
}

#[rstest]
fn dispatch_returns_none_for_unknown_command() {
    assert!(run_dispatch(vec![], "Unknown", serde_json::json!({})).is_none());
}

#[rstest]
fn dispatch_calls_registered_handler() {
    let cmd = ControlCommand {
        name: "Ping",
        handler: Box::new(|_payload, _ctx| serde_json::json!({"type": "Pong"})),
    };
    assert_eq!(
        run_dispatch(vec![cmd], "Ping", serde_json::json!({})),
        Some(serde_json::json!({"type": "Pong"}))
    );
}

#[rstest]
fn dispatch_passes_payload_to_handler() {
    let cmd = ControlCommand {
        name: "Echo",
        handler: Box::new(|payload, _ctx| payload),
    };
    let input = serde_json::json!({"data": 42});
    assert_eq!(run_dispatch(vec![cmd], "Echo", input.clone()), Some(input));
}
