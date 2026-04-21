use nyne::prelude::*;
use nyne::router::Provider;

use crate::provider::FsProvider;

struct FsPlugin;

impl Plugin for FsPlugin {
    fn id(&self) -> &'static str { "fs" }

    fn providers(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<Arc<dyn Provider>>> {
        Ok(vec![Arc::new(FsProvider {
            fs: Arc::clone(ctx.fs()),
        })])
    }
}

nyne::register_plugin!(FsPlugin);
