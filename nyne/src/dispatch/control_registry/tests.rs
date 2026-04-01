use super::*;
use crate::plugin::control::{ControlCommand, ProcessTable};
use crate::test_support::stub_activation_context;

fn test_context() -> (crate::dispatch::activation::ActivationContext, ProcessTable) {
    let ctx = stub_activation_context();
    let procs: ProcessTable = Default::default();
    (ctx, procs)
}

#[test]
fn dispatch_returns_none_for_unknown_command() {
    let registry = ControlRegistry::from_commands(vec![]);
    let (ctx, procs) = test_context();
    let ctrl_ctx = ControlContext {
        activation: &ctx,
        processes: &procs,
    };
    assert!(registry.dispatch("Unknown", serde_json::json!({}), &ctrl_ctx).is_none());
}

#[test]
fn dispatch_calls_registered_handler() {
    let cmd = ControlCommand {
        name: "Ping",
        handler: Box::new(|_payload, _ctx| serde_json::json!({"type": "Pong"})),
    };
    let registry = ControlRegistry::from_commands(vec![cmd]);
    let (ctx, procs) = test_context();
    let ctrl_ctx = ControlContext {
        activation: &ctx,
        processes: &procs,
    };
    let result = registry.dispatch("Ping", serde_json::json!({}), &ctrl_ctx);
    assert_eq!(result, Some(serde_json::json!({"type": "Pong"})));
}

#[test]
fn dispatch_passes_payload_to_handler() {
    let cmd = ControlCommand {
        name: "Echo",
        handler: Box::new(|payload, _ctx| payload),
    };
    let registry = ControlRegistry::from_commands(vec![cmd]);
    let (ctx, procs) = test_context();
    let ctrl_ctx = ControlContext {
        activation: &ctx,
        processes: &procs,
    };
    let input = serde_json::json!({"data": 42});
    let result = registry.dispatch("Echo", input.clone(), &ctrl_ctx);
    assert_eq!(result, Some(input));
}
