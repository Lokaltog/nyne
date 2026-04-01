//! Plugin registration and lifecycle for the nyne meta provider.

use std::sync::Arc;
use std::time::Instant;

use nyne::prelude::*;
use nyne::templates::{HandleBuilder, TemplateHandle};

use crate::provider::NyneProvider;

struct NynePlugin;

impl Plugin for NynePlugin {
    nyne::provider_graph!(NyneProvider);

    fn id(&self) -> &'static str { "nyne" }

    fn providers(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<Arc<dyn Provider>>> {
        let mut b = HandleBuilder::new();
        let key = b.register("nyne/nyne", include_str!("provider/templates/nyne.md.j2"));
        Ok(vec![Arc::new(NyneProvider {
            template: TemplateHandle::new(&b.finish(), key),
            ctx: Arc::clone(ctx),
            start_time: Instant::now(),
        })])
    }
}

#[linkme::distributed_slice(PLUGINS)]
fn nyne_plugin() -> Box<dyn Plugin> { Box::new(NynePlugin) }
