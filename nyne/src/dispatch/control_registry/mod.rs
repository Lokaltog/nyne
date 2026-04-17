//! Registry of plugin-provided control commands, indexed by name.
//!
//! Built once at mount time from pre-collected [`ControlCommand`]s.
//! The control server dispatches incoming requests whose `type` field does not
//! match a core command by looking up a handler here.
//!
//! Backed by [`NamedRegistry`] — collisions are warned at registration
//! time and last-registration wins.

use super::named_registry::NamedRegistry;
use crate::plugin::control::{ControlCommand, ControlContext, ControlHandler};

/// Registry of plugin control commands, keyed by command name.
///
/// Immutable after construction — commands cannot be added at runtime.
/// Lookup is O(1).
pub struct ControlRegistry {
    commands: NamedRegistry<&'static str, ControlHandler>,
}

impl ControlRegistry {
    /// Build the registry from pre-collected control commands.
    pub(crate) fn from_commands(commands: Vec<ControlCommand>) -> Self {
        Self {
            commands: NamedRegistry::from_entries(
                "control command",
                commands.into_iter().map(|cmd| (cmd.name, cmd.handler)),
            ),
        }
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
