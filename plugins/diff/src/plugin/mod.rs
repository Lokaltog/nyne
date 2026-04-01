use linkme::distributed_slice;
use nyne::plugin::PluginFactory;
use nyne::prelude::*;
use nyne::router::Provider;

use crate::provider::DiffProvider;

struct DiffPlugin;

impl Plugin for DiffPlugin {
    fn id(&self) -> &'static str { "diff" }

    fn providers(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<Arc<dyn Provider>>> {
        Ok(vec![Arc::new(DiffProvider {
            root_prefix: ctx.root_prefix().to_owned(),
        })])
    }
}

#[allow(unsafe_code)]
#[distributed_slice(PLUGINS)]
static DIFF_PLUGIN: PluginFactory = || Box::new(DiffPlugin);
