//! Plugin registration and lifecycle implementation.

use std::sync::Arc;

use color_eyre::eyre::Result;
use linkme::distributed_slice;
use nyne::dispatch::activation::ActivationContext;
use nyne::plugin::{PLUGINS, Plugin, PluginFactory};
use nyne::provider::Provider;
use tracing::info;

use crate::analysis::AnalysisEngine;
use crate::config::AnalysisConfig;
use crate::providers::AnalysisProvider;

/// Entry point for the analysis plugin.
struct AnalysisPlugin;

impl Plugin for AnalysisPlugin {
    fn id(&self) -> &'static str { "analysis" }

    fn activate(&self, ctx: &mut ActivationContext) -> Result<()> {
        let config = AnalysisConfig::from_plugin_config(ctx.plugin_config("analysis"));
        let engine = Arc::new(AnalysisEngine::build_filtered(&config));

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
