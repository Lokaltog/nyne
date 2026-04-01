//! Registry of plugin-provided control commands, indexed by name.
//!
//! Built once at mount time from pre-collected [`ControlCommand`]s.
//! The control server dispatches incoming requests whose `type` field does not
//! match a core command by looking up a handler here.

use std::collections::HashMap;

use tracing::warn;

use crate::plugin::control::{ControlCommand, ControlContext, ControlHandler};

/// Registry of plugin control commands, keyed by command name.
///
/// Immutable after construction — commands cannot be added at runtime.
/// Lookup is O(1) via `HashMap`.
pub struct ControlRegistry {
    commands: HashMap<&'static str, ControlHandler>,
}

impl ControlRegistry {
    /// Build the registry from pre-collected control commands.
    ///
    /// Duplicate command names are logged as warnings; last registration wins.
    pub(crate) fn from_commands(commands: Vec<ControlCommand>) -> Self {
        let mut map = HashMap::new();
        for cmd in commands {
            if map.insert(cmd.name, cmd.handler).is_some() {
                warn!(command = cmd.name, "duplicate control command");
            }
        }
        Self { commands: map }
    }

    /// Dispatch a plugin control command by name.
    ///
    /// Returns `None` if no handler is registered for the given name.
    pub(crate) fn dispatch(
        &self,
        name: &str,
        payload: serde_json::Value,
        ctx: &ControlContext<'_>,
    ) -> Option<serde_json::Value> {
        Some(self.commands.get(name)?(payload, ctx))
    }
}

#[cfg(test)]
mod tests;
