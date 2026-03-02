use std::sync::Arc;

use color_eyre::eyre::Result;
use linkme::distributed_slice;

use crate::dispatch::activation::ActivationContext;
use crate::plugin::{PLUGINS, Plugin, PluginFactory};
use crate::provider::Provider;

/// Plugin wrapping the three core providers that always ship with nyne.
pub struct CorePlugin;

impl Plugin for CorePlugin {
    fn id(&self) -> &'static str { "core" }

    fn providers(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<Arc<dyn Provider>>> {
        Ok(vec![
            Arc::new(super::companion::CompanionProvider::new(Arc::clone(ctx))),
            Arc::new(super::directory::DirectoryProvider::new(Arc::clone(ctx))),
            Arc::new(super::nyne::NyneProvider::new(ctx)),
        ])
    }
}

#[allow(unsafe_code)]
#[distributed_slice(PLUGINS)]
static CORE_PLUGIN: PluginFactory = || Box::new(CorePlugin);
