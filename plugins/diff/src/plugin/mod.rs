use nyne::prelude::*;
use nyne::router::Provider;

use crate::provider::DiffProvider;

struct DiffPlugin;

impl Plugin for DiffPlugin {
    fn id(&self) -> &'static str { "diff" }

    fn providers(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<Arc<dyn Provider>>> {
        Ok(vec![Arc::new(DiffProvider {
            root: ctx.root().to_path_buf(),
        })])
    }
}

nyne::register_plugin!(DiffPlugin);
