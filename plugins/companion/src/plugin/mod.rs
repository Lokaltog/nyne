pub mod config;
use linkme::distributed_slice;
use nyne::plugin::PluginFactory;
use nyne::prelude::*;
use nyne::router::{Provider, RouteTree};

use crate::context::CompanionContextExt;
use crate::plugin::config::CompanionConfig;
use crate::provider::CompanionProvider;

struct CompanionPlugin;

impl Plugin for CompanionPlugin {
    nyne::plugin_config!(CompanionConfig);

    fn id(&self) -> &'static str { "companion" }

    fn activate(&self, ctx: &mut ActivationContext) -> Result<()> {
        ctx.companion_extensions_mut();
        Ok(())
    }

    #[expect(clippy::expect_used, reason = "companion extension point is a lifecycle invariant")]
    fn providers(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<Arc<dyn Provider>>> {
        let config = CompanionConfig::from_context(ctx, self.id());
        let ext = ctx.companion_extensions().expect("CompanionExtensions missing");
        Ok(vec![Arc::new(CompanionProvider {
            suffix: config.suffix.into(),
            file_tree: RouteTree::builder().apply(&ext.file).build(),
            dir_tree: RouteTree::builder().apply(&ext.dir).build(),
            mount_tree: RouteTree::builder().apply(&ext.mount).build(),
            fs: Arc::clone(ctx.fs()),
        })])
    }
}

#[allow(unsafe_code)]
#[distributed_slice(PLUGINS)]
static COMPANION_PLUGIN: PluginFactory = || Box::new(CompanionPlugin);
