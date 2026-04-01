//! SetVisibility control command handler.
//!
//! Handles `SetVisibility` requests from the control socket, allowing
//! runtime changes to per-PID and per-name visibility rules.

use std::collections::HashMap;
use std::sync::Arc;

use nyne::ControlContext;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::process_visibility::ProcessVisibility;
use crate::visibility_map::VisibilityMap;

/// Identifies the subject of a visibility change — either a specific PID or a
/// process name pattern.
///
/// Wire format (untagged): `{"pid": 123}` or `{"name": "fish"}` or `{}` (query-only).
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum VisibilityTarget {
    /// Target a specific attached process by PID.
    Pid { pid: i32 },
    /// Target processes matching a comm name.
    Name { name: String },
    /// No target — query current rules without changing anything.
    Query {},
}

/// Current visibility rule state returned by `SetVisibility` responses.
#[derive(Debug, Serialize, Deserialize)]
pub struct VisibilityRules {
    /// Per-PID explicit overrides (only registered processes).
    pub pid_rules: Vec<PidVisibility>,
    /// Dynamic name-based rules set at runtime.
    pub name_rules: Vec<NameVisibility>,
}

/// Visibility override for a specific PID.
#[derive(Debug, Serialize, Deserialize)]
pub struct PidVisibility {
    pub pid: i32,
    pub command: String,
    pub visibility: ProcessVisibility,
}

/// Visibility rule for a process comm name.
#[derive(Debug, Serialize, Deserialize)]
pub struct NameVisibility {
    pub name: String,
    pub visibility: ProcessVisibility,
}

/// Wire-format request for the `SetVisibility` control command.
#[derive(Debug, Deserialize)]
struct SetVisibilityRequest {
    #[serde(flatten)]
    target: VisibilityTarget,
    #[serde(default)]
    visibility: Option<ProcessVisibility>,
}

/// Handle a `SetVisibility` control command.
///
/// Returns a JSON response with the current visibility rules snapshot.
/// If no visibility map is available (plugin not fully initialized),
/// returns an error payload.
pub fn handle_set_visibility(
    payload: serde_json::Value,
    ctrl_ctx: &ControlContext<'_>,
    vis: Option<&Arc<VisibilityMap>>,
) -> serde_json::Value {
    let Some(map) = vis else {
        return serde_json::json!({"type": "Error", "message": "visibility plugin not initialized"});
    };

    let req: SetVisibilityRequest = match serde_json::from_value(payload) {
        Ok(r) => r,
        Err(e) => return serde_json::json!({"type": "Error", "message": format!("{e:#}")}),
    };

    let vis = req.visibility.unwrap_or(ProcessVisibility::Default);

    match req.target {
        VisibilityTarget::Pid { pid } => {
            let processes = ctrl_ctx.processes.lock();
            if !processes.iter().any(|p| p.pid == pid) {
                return serde_json::json!({
                    "type": "Error",
                    "message": format!("PID {pid} is not a registered process (use ListProcesses to see registered PIDs)")
                });
            }
            drop(processes);
            map.set_pid(pid.cast_unsigned(), vis);
            debug!(pid, %vis, "process visibility set");
        }
        VisibilityTarget::Name { name } => {
            debug!(name = %name, %vis, "name visibility rule set");
            map.set_name_rule(&name, vis);
        }
        VisibilityTarget::Query {} => {
            debug!("visibility rules queried");
        }
    }

    serde_json::json!({
        "type": "Visibility",
        "rules": build_visibility_rules(ctrl_ctx, map),
    })
}

/// Snapshot all active visibility rules into a response struct.
fn build_visibility_rules(ctrl_ctx: &ControlContext<'_>, map: &VisibilityMap) -> VisibilityRules {
    let table = ctrl_ctx.processes.lock();
    let pid_entries: HashMap<u32, _> = map.explicit_pid_entries().into_iter().collect();

    let pid_rules = table
        .iter()
        .filter_map(|proc| {
            let &vis = pid_entries.get(&proc.pid.cast_unsigned())?;
            Some(PidVisibility {
                pid: proc.pid,
                command: proc.command.clone(),
                visibility: vis,
            })
        })
        .collect();

    let name_rules = map
        .dynamic_name_rules()
        .into_iter()
        .map(|(name, visibility)| NameVisibility { name, visibility })
        .collect();

    VisibilityRules { pid_rules, name_rules }
}
