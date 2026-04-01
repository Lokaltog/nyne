use linkme::distributed_slice;
use nyne::plugin::PluginFactory;
use nyne::prelude::*;
use nyne::router::{GenerationMap, Provider};

use crate::provider::CacheProvider;

struct CachePlugin;

impl Plugin for CachePlugin {
    fn id(&self) -> &'static str { "cache" }

    fn providers(&self, _ctx: &Arc<ActivationContext>) -> Result<Vec<Arc<dyn Provider>>> {
        Ok(vec![Arc::new(CacheProvider::new(Arc::new(GenerationMap::new())))])
    }
}

#[allow(unsafe_code)]
#[distributed_slice(PLUGINS)]
static CACHE_PLUGIN: PluginFactory = || Box::new(CachePlugin);
