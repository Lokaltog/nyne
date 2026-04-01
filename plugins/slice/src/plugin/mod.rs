use linkme::distributed_slice;
use nyne::plugin::PluginFactory;
use nyne::prelude::*;
use nyne::router::Provider;

use crate::provider::SliceProvider;

struct SlicePlugin;

impl Plugin for SlicePlugin {
    fn id(&self) -> &'static str { "slice" }

    fn providers(&self, _ctx: &Arc<ActivationContext>) -> Result<Vec<Arc<dyn Provider>>> {
        Ok(vec![Arc::new(SliceProvider)])
    }
}

#[allow(unsafe_code)]
#[distributed_slice(PLUGINS)]
static SLICE_PLUGIN: PluginFactory = || Box::new(SlicePlugin);
