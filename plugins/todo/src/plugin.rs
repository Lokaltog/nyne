//! Plugin registration and lifecycle implementation.

use linkme::distributed_slice;
use nyne::plugin::PluginFactory;
use nyne::prelude::*;
use tracing::info;

use crate::config::Config;
use crate::provider::TodoProvider;

/// Entry point for the todo plugin.
struct TodoPlugin;

impl Plugin for TodoPlugin {
    fn id(&self) -> &'static str { "todo" }

    fn providers(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<Arc<dyn Provider>>> {
        let config = Config::from_plugin_config(ctx.plugin_config("todo"));
        info!("todo plugin activated");
        Ok(vec![Arc::new(TodoProvider::new(Arc::clone(ctx), config))])
    }
}

/// Link-time registration of the todo plugin into the global `PLUGINS` slice.
#[allow(unsafe_code)]
#[distributed_slice(PLUGINS)]
static TODO_PLUGIN: PluginFactory = || Box::new(TodoPlugin);
