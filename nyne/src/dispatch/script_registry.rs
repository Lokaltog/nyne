use std::collections::HashMap;
use std::sync::Arc;

use color_eyre::eyre::{Result, eyre};
use tracing::warn;

use super::script::{Script, ScriptContext};
use crate::dispatch::activation::ActivationContext;
use crate::plugin::PLUGINS;

/// Registry of named scripts, indexed by dotted address.
///
/// Built at startup by iterating [`PLUGINS`] and calling each plugin's
/// [`scripts`](crate::plugin::Plugin::scripts) method.
pub struct ScriptRegistry {
    scripts: HashMap<String, Arc<dyn Script>>,
}

/// Script discovery, registration, and execution.
impl ScriptRegistry {
    /// Build the registry from all plugin-provided scripts.
    #[allow(clippy::excessive_nesting)] // warn! macro expansion inside for+match
    pub(crate) fn new(ctx: &Arc<ActivationContext>) -> Self {
        let mut scripts = HashMap::new();
        for factory in PLUGINS {
            let plugin = factory();
            let entries = match plugin.scripts(ctx) {
                Ok(entries) => entries,
                Err(e) => {
                    warn!(plugin = plugin.id(), error = %e, "plugin script creation failed");
                    continue;
                }
            };
            for (address, script) in entries {
                if scripts.insert(address.clone(), script).is_none() {
                    continue;
                }
                warn!(plugin = plugin.id(), address = %address, "duplicate script address");
            }
        }
        Self { scripts }
    }

    /// Execute a script by its fully-qualified address.
    pub(crate) fn exec(&self, address: &str, ctx: &ScriptContext<'_>, stdin: &[u8]) -> Result<Vec<u8>> {
        let script = self
            .scripts
            .get(address)
            .ok_or_else(|| eyre!("unknown script: {address}"))?;
        script.exec(ctx, stdin)
    }
}
