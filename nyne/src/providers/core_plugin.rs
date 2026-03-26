//! Plugin registration for the three built-in core providers.
//!
//! Registers [`CorePlugin`] in the global [`PLUGINS`](crate::plugin::PLUGINS)
//! distributed slice so the dispatch layer discovers it alongside external
//! plugin crates. This keeps the plugin discovery mechanism uniform -- core
//! providers go through the same activation path as plugin providers.

use linkme::distributed_slice;

use crate::plugin::PluginFactory;
use crate::prelude::*;

/// Plugin wrapping the three core providers that always ship with nyne.
pub struct CorePlugin;

/// Plugin implementation for the core providers.
impl Plugin for CorePlugin {
    /// Returns the core plugin identifier.
    fn id(&self) -> &'static str { "core" }

    /// Creates the three core providers: companion, directory, and nyne.
    fn providers(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<Arc<dyn Provider>>> {
        Ok(vec![
            Arc::new(super::companion::CompanionProvider::new(Arc::clone(ctx))),
            Arc::new(super::directory::DirectoryProvider::new(Arc::clone(ctx))),
            Arc::new(super::nyne::NyneProvider::new(ctx)),
        ])
    }
}

#[allow(unsafe_code)]
#[distributed_slice(PLUGINS)]
/// Core plugin factory registered in the global plugin slice.
static CORE_PLUGIN: PluginFactory = || Box::new(CorePlugin);
