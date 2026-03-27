//! Plugin registration and lifecycle implementation.

use linkme::distributed_slice;
use nyne::plugin::PluginFactory;
use nyne::prelude::*;
use tracing::info;

use crate::config::Config;
use crate::engine::Engine;
use crate::providers::AnalysisProvider;

/// Entry point for the analysis plugin.
struct AnalysisPlugin;

/// Plugin lifecycle for the analysis engine.
///
/// During activation, reads `[plugin.analysis]` config, builds a filtered
/// [`Engine`] respecting enabled/disabled rules, and inserts it into
/// the `TypeMap` as `Arc<Engine>`. The provider phase creates an
/// [`AnalysisProvider`] that contributes `ANALYSIS.md` to symbol directories.
impl Plugin for AnalysisPlugin {
    fn id(&self) -> &'static str { "analysis" }

    fn activate(&self, ctx: &mut ActivationContext) -> Result<()> {
        let config = Config::from_plugin_config(ctx.plugin_config("analysis"));
        let engine = Arc::new(Engine::build_filtered(&config));

        info!(
            enabled = config.enabled,
            rules = ?config.rules,
            "analysis plugin activated",
        );

        ctx.insert(engine);
        Ok(())
    }

    fn providers(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<Arc<dyn Provider>>> {
        Ok(vec![Arc::new(AnalysisProvider::new(Arc::clone(ctx)))])
    }
}

/// Link-time registration of the analysis plugin into the global `PLUGINS` slice.
#[allow(unsafe_code)]
#[distributed_slice(PLUGINS)]
static ANALYSIS_PLUGIN: PluginFactory = || Box::new(AnalysisPlugin);
