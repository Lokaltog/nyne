//! Plugin-provided control commands for the IPC control socket.
//!
//! Plugins register named control commands via [`Plugin::control_commands`].
//! The control server dispatches plugin commands by name — unknown commands
//! are rejected, core commands (Exec, Register, etc.) are handled directly.

use std::sync::Arc;
use std::time::SystemTime;

use parking_lot::Mutex;

use crate::dispatch::activation::ActivationContext;

/// Shared state for tracking attached processes.
///
/// Shared between the server loop thread and request handlers. Entries are
/// added on `Register`, removed on `Unregister`, and pruned of dead PIDs
/// on `ListProcesses`.
pub type ProcessTable = Arc<Mutex<Vec<AttachedProcess>>>;

/// A process currently attached to the sandbox via `nyne attach`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AttachedProcess {
    pub pid: i32,
    pub command: String,
    pub start_time: SystemTime,
}

/// Context available to control command handlers.
///
/// Provides access to the attached process table (shared with Register/
/// Unregister handling) and the activation context.
pub struct ControlContext<'a> {
    pub activation: &'a ActivationContext,
    pub processes: &'a ProcessTable,
}

/// Handler function type for plugin control commands.
///
/// Receives the raw JSON payload (the full request minus the `type` field)
/// and returns a JSON response value.
pub type ControlHandler = Box<dyn Fn(serde_json::Value, &ControlContext<'_>) -> serde_json::Value + Send + Sync>;

/// A plugin-provided control command, registered by name.
///
/// The control server deserializes the `type` field from the incoming JSON
/// request. If it does not match a core command, the server looks up a
/// plugin command handler by name and passes the raw JSON payload.
pub struct ControlCommand {
    /// Command name, matching the `type` field in the wire JSON.
    pub name: &'static str,
    /// Handler receiving the raw JSON payload and returning a JSON response value.
    pub handler: ControlHandler,
}
