//! Registry of activated providers and scripts.

use std::path::Path;

use tracing::warn;

use crate::config::NyneConfig;
use crate::prelude::*;
use crate::process::Spawner;
use crate::types::real_fs::RealFs;

/// Registry of activated providers, built once per mount session.
///
/// Constructed via the two-phase plugin lifecycle in [`default_for`](Self::default_for):
/// first all plugins insert shared services into `ActivationContext`, then each
/// plugin creates its providers. Only providers that pass
/// [`should_activate`](crate::provider::Provider::should_activate) are retained.
///
/// The registry is immutable after construction -- the router and resolve
/// pipeline iterate over `active_providers()` without locking.
pub struct ProviderRegistry {
    active: Vec<Arc<dyn Provider>>,
}

/// Provider discovery, activation, and lookup.
impl ProviderRegistry {
    /// Create an empty registry with no providers (test-only).
    #[cfg(test)]
    pub fn empty() -> Self { Self { active: vec![] } }

    /// Return only the providers that passed activation.
    pub fn active_providers(&self) -> &[Arc<dyn Provider>] { &self.active }

    /// Find an active provider by its ID.
    pub fn find_provider(&self, id: ProviderId) -> Option<&Arc<dyn Provider>> {
        self.active.iter().find(|p| p.id() == id)
    }

    /// Build a registry from a pre-built activation context.
    ///
    /// Discovers providers from the [`PLUGINS`] distributed slice.
    /// Each plugin's [`providers`](crate::plugin::Plugin::providers) method
    /// is called and results are filtered by [`should_activate`](crate::provider::Provider::should_activate).
    pub fn from_context(ctx: &Arc<ActivationContext>) -> Self {
        let mut active: Vec<Arc<dyn Provider>> = Vec::new();
        for factory in PLUGINS {
            let plugin = factory();
            match plugin.providers(ctx) {
                Ok(providers) => active.extend(providers),
                Err(e) => {
                    warn!(plugin = plugin.id(), error = %e, "plugin provider creation failed");
                }
            }
        }

        active.retain(|p| p.should_activate(ctx));
        Self { active }
    }

    /// Build a registry with the default set of providers, activated
    /// against the given root directory.
    ///
    /// Runs the two-phase plugin lifecycle:
    /// 1. All plugins insert services via [`Plugin::activate`](crate::plugin::Plugin::activate)
    /// 2. Context is frozen in `Arc`, all plugins create providers
    ///
    /// Also returns the shared `ActivationContext` for use by other
    /// registries (e.g., `ScriptRegistry`).
    pub fn default_for(
        host_root: &Path,
        root: &Path,
        overlay_root: &Path,
        real_fs: Arc<dyn RealFs>,
        config: Arc<NyneConfig>,
        spawner: Arc<Spawner>,
    ) -> (Self, Arc<ActivationContext>) {
        let mut ctx = ActivationContext::new(
            host_root.to_owned(),
            root.to_owned(),
            overlay_root.to_owned(),
            real_fs,
            config,
            spawner,
        );

        // Phase 1: all plugins insert services into the mutable context.
        for factory in PLUGINS {
            let plugin = factory();
            if let Err(e) = plugin.activate(&mut ctx) {
                warn!(plugin = plugin.id(), error = %e, "plugin activation failed");
            }
        }

        // Freeze context — no more mutations after this point.
        let ctx = Arc::new(ctx);

        // Phase 2: from_context collects providers from plugins.
        (Self::from_context(&ctx), ctx)
    }
}
