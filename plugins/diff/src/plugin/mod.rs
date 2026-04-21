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

nyne::register_plugin!(DiffPlugin);
