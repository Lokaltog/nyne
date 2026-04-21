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

nyne::register_plugin!(SlicePlugin);
