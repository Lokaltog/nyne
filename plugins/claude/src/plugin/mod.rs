//! Plugin registration and lifecycle implementation.
pub mod config;

use std::sync::Arc;

use color_eyre::eyre::Result;
use linkme::distributed_slice;
use nyne::config::PluginConfig;
use nyne::plugin::{PLUGINS, Plugin, PluginFactory};
use nyne::router::Provider;
use nyne::{ActivationContext, ScriptEntry};
use nyne_source::dominant_ext;

use crate::plugin::config::Config;
use crate::provider::hooks::script_entries;
use crate::provider::{ClaudeProvider, build_templates, routes};

/// Entry point for the claude plugin.
struct ClaudePlugin;

/// Plugin lifecycle for Claude Code integration.
///
/// Registers hook scripts (pre/post tool use, session start, stop, statusline)
/// and creates the [`ClaudeProvider`] which contributes the `.claude/` virtual
/// directory with settings, skills, and system prompt.
impl Plugin for ClaudePlugin {
    nyne::provider_graph!(ClaudeProvider);

    nyne::plugin_config!(Config);

    fn id(&self) -> &'static str { "claude" }

    /// Register hook scripts based on per-hook toggle configuration.
    fn scripts(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<ScriptEntry>> {
        Ok(script_entries(&Config::from_context(ctx, self.id())))
    }

    fn providers(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<Arc<dyn Provider>>> {
        Ok(vec![Arc::new(ClaudeProvider {
            root: ctx.root().to_owned(),
            root_tree: routes::build_root_tree(),
            at_tree: routes::build_at_tree(),
            templates: build_templates(),
            ext: dominant_ext(ctx),
            config: Config::from_context(ctx, self.id()),
        })])
    }
}

/// Link-time registration of the claude plugin into the global `PLUGINS` slice.
#[allow(unsafe_code)]
#[distributed_slice(PLUGINS)]
static CLAUDE_PLUGIN: PluginFactory = || Box::new(ClaudePlugin);
