//! Plugin registration and lifecycle implementation.

use linkme::distributed_slice;
use nyne::config::NyneConfig;
use nyne::dispatch::script::ScriptEntry;
use nyne::plugin::PluginFactory;
use nyne::prelude::*;
use tracing::info;

use crate::config::ClaudePluginConfig;
use crate::provider::{ClaudeProvider, script_entries};

/// Entry point for the claude plugin.
struct ClaudePlugin;

impl Plugin for ClaudePlugin {
    fn id(&self) -> &'static str { "claude" }

    fn scripts(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<ScriptEntry>> {
        Ok(script_entries(&ClaudePluginConfig::from_plugin_config(
            ctx.plugin_config("claude"),
        )))
    }

    fn providers(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<Arc<dyn Provider>>> {
        let config = ClaudePluginConfig::from_plugin_config(ctx.plugin_config("claude"));
        info!("claude plugin activated");
        Ok(vec![Arc::new(ClaudeProvider::new(Arc::clone(ctx), config))])
    }

    fn default_config(&self) -> Option<toml::Table> { toml::Table::try_from(ClaudePluginConfig::default()).ok() }

    fn resolved_config(&self, config: &NyneConfig) -> Option<serde_json::Value> {
        serde_json::to_value(ClaudePluginConfig::from_plugin_config(config.plugin.get("claude"))).ok()
    }
}

/// Link-time registration of the claude plugin into the global `PLUGINS` slice.
#[allow(unsafe_code)]
#[distributed_slice(PLUGINS)]
static CLAUDE_PLUGIN: PluginFactory = || Box::new(ClaudePlugin);
