pub mod config;
use std::sync::Arc;

use nyne::path_filter::PathFilter;
use nyne::prelude::*;
use nyne::router::{Provider, RouteTree};

use crate::context::CompanionContextExt;
use crate::extensions::CompanionExtensions;
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

    fn providers(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<Arc<dyn Provider>>> {
        let config = ctx.plugin_config::<CompanionConfig>(self.id());
        let ext = ctx.require_service::<CompanionExtensions>()?;
        Ok(vec![Arc::new(CompanionProvider {
            suffix: config.suffix.into(),
            file_tree: RouteTree::builder().apply(&ext.file).build(),
            dir_tree: RouteTree::builder().apply(&ext.dir).build(),
            mount_tree: RouteTree::builder().apply(&ext.mount).build(),
            fs: Arc::clone(ctx.fs()),
            path_filter: ctx.get::<Arc<PathFilter>>().cloned(),
        })])
    }
}

nyne::register_plugin!(CompanionPlugin);
