//! Plugin discovery and lifecycle.
//!
//! A plugin bundles related VFS providers and their supporting services.
//! Plugins are discovered at link time via the [`PLUGINS`] distributed slice
//! and activated in two phases during mount:
//!
//! 1. **[`activate`](Plugin::activate)** — each plugin inserts shared services
//!    into the [`ActivationContext`] `TypeMap`. All activations complete before
//!    any providers are created.
//! 2. **[`providers`](Plugin::providers)** — each plugin creates its provider
//!    instances, reading services from the now-frozen context.
//!
//! # Registration
//!
//! ```ignore
//! use nyne::plugin::{Plugin, PLUGINS};
//!
//! struct MyPlugin;
//!
//! impl Plugin for MyPlugin {
//!     fn id(&self) -> &str { "my-plugin" }
//!     fn providers(
//!         &self,
//!         ctx: &Arc<ActivationContext>,
//!     ) -> Result<Vec<Arc<dyn Provider>>> {
//!         Ok(vec![])
//!     }
//! }
//!
//! #[linkme::distributed_slice(PLUGINS)]
//! fn my_plugin() -> Box<dyn Plugin> {
//!     Box::new(MyPlugin)
//! }
//! ```

use std::sync::Arc;

use color_eyre::eyre::Result;
use linkme::distributed_slice;

use crate::config::NyneConfig;
use crate::dispatch::activation::ActivationContext;
use crate::dispatch::script::ScriptEntry;
use crate::provider::Provider;

/// A plugin bundles related VFS providers and their supporting services.
///
/// Plugins insert shared services into [`ActivationContext`] during
/// [`activate`](Plugin::activate), then create providers in
/// [`providers`](Plugin::providers) after all plugins have activated.
///
/// There are no ordering guarantees between plugin activations — providers
/// must handle missing services gracefully (capability degradation).
pub trait Plugin: Send + Sync {
    /// Unique identifier for this plugin.
    ///
    /// Used for config lookup (`[plugin.<id>]`) and diagnostics.
    fn id(&self) -> &str;

    /// Phase 1: Insert services into the context's `TypeMap`.
    ///
    /// Called once per mount, before any providers are created. The context
    /// is mutable — insert shared services here via
    /// [`ActivationContext::insert`].
    ///
    /// Returning an error aborts the mount.
    fn activate(&self, ctx: &mut ActivationContext) -> Result<()> {
        let _ = ctx;
        Ok(())
    }

    /// Phase 2: Create provider instances.
    ///
    /// Called after ALL plugins have run [`activate`](Plugin::activate), so
    /// all `TypeMap` services are available. The context is now wrapped in
    /// `Arc` — clone it into providers that need shared ownership.
    fn providers(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<Arc<dyn Provider>>>;

    /// Named scripts this plugin provides.
    ///
    /// Called after [`providers`](Plugin::providers). Scripts are addressable
    /// as `provider.<plugin-id>.<name>`.
    fn scripts(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<ScriptEntry>> {
        let _ = ctx;
        Ok(vec![])
    }

    /// Return the fully-resolved plugin configuration as a TOML value.
    ///
    /// Plugins that deserialize a config struct from `[plugin.<id>]` should
    /// serialize it back here with all defaults filled in, so `nyne config`
    /// can show the effective configuration.
    ///
    /// Returns `None` (default) if the plugin has no configuration.
    fn resolved_config(&self, config: &NyneConfig) -> Option<toml::Value> {
        let _ = config;
        None
    }
}

/// Plugin factory function type for the distributed slice.
pub type PluginFactory = fn() -> Box<dyn Plugin>;

/// Link-time distributed slice of plugin factories.
///
/// Each plugin crate contributes one entry via `#[distributed_slice(PLUGINS)]`.
/// At mount time, the registry iterates this slice to discover all plugins.
#[allow(unsafe_code)]
#[distributed_slice]
pub static PLUGINS: [PluginFactory];
