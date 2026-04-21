//! Plugin discovery and lifecycle.
//!
//! A plugin bundles related VFS providers and their supporting services.
//! Plugins are discovered at link time via the [`PLUGINS`] distributed slice
//! and activated during mount via [`Plugin::activate`], which inserts shared
//! services and router providers into the [`ActivationContext`] `AnyMap`.
//!
//! # Registration
//!
//! ```ignore
//! use nyne::plugin::Plugin;
//!
//! struct MyPlugin;
//!
//! impl Plugin for MyPlugin {
//!     fn id(&self) -> &str { "my-plugin" }
//!     fn activate(&self, ctx: &mut ActivationContext) -> Result<()> {
//!         Ok(())
//!     }
//! }
//!
//! nyne::register_plugin!(MyPlugin);
//! ```

use linkme::distributed_slice;

pub use self::config::PluginConfig;
use crate::config::NyneConfig;
use crate::dispatch::script::ScriptEntry;
use crate::prelude::*;
use crate::router::Provider;

/// Everything a plugin contributes after activation.
///
/// Returned by [`Plugin::contributions`] to collect providers, scripts, and
/// control commands in a single pass. Built from the individual trait methods
/// by default — plugins rarely need to override `contributions()` directly.
pub struct PluginContributions {
    /// Providers for the middleware chain.
    pub providers: Vec<Arc<dyn Provider>>,
    /// Named scripts addressable via `nyne exec`.
    pub scripts: Vec<ScriptEntry>,
    /// Control commands handled via the IPC control socket.
    pub control_commands: Vec<ControlCommand>,
}

pub mod config;
pub mod control;
pub use control::{AttachedProcess, ControlCommand, ControlContext, ProcessTable};

/// A plugin bundles related VFS providers and their supporting services.
///
/// Plugins insert shared services into [`ActivationContext`] during
/// [`activate`](Plugin::activate). Providers are deposited into the
/// activation context's `AnyMap` during activation.
///
/// Plugins are activated in dependency order — [`provider_graph`](Plugin::provider_graph)
/// declares which providers this plugin owns and their dependencies. The
/// activation loop topologically sorts plugins before calling
/// [`activate`](Plugin::activate).
pub trait Plugin: Send + Sync {
    /// Unique identifier for this plugin.
    ///
    /// Used for config lookup (`[plugin.<id>]`) and diagnostics. This is
    /// intentionally separate from [`ProviderId`] — a plugin ID names the
    /// *plugin* (one per crate), while provider IDs name the *middleware
    /// providers* the plugin contributes (one or more per plugin).
    fn id(&self) -> &str;

    /// Provider ownership and dependency graph for activation ordering.
    ///
    /// Each entry is `(provider_id, dependencies)` — the provider IDs this
    /// plugin owns paired with their compile-time dependency lists. The
    /// activation loop builds a plugin dependency graph from this and
    /// topologically sorts before calling [`activate`](Plugin::activate).
    fn provider_graph(&self) -> &[(ProviderId, &[ProviderId])] { &[] }

    /// Insert services and providers into the context's `AnyMap`.
    ///
    /// Called once per mount. The context is mutable — insert shared
    /// services and router providers here via [`ActivationContext::insert`].
    ///
    /// Returning an error aborts the mount.
    fn activate(&self, ctx: &mut ActivationContext) -> Result<()> {
        let _ = ctx;
        Ok(())
    }

    /// Providers this plugin contributes to the middleware chain.
    ///
    /// Called after [`activate`](Self::activate). Providers are ordered by the
    /// chain's dependency-graph sort — see [`crate::router::Chain::build`].
    fn providers(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<Arc<dyn Provider>>> {
        let _ = ctx;
        Ok(vec![])
    }

    /// Named scripts this plugin provides.
    ///
    /// Scripts are addressable as `provider.<plugin-id>.<name>`.
    fn scripts(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<ScriptEntry>> {
        let _ = ctx;
        Ok(vec![])
    }

    /// Control commands this plugin handles via the IPC control socket.
    ///
    /// Called after [`activate`](Self::activate). Each command is registered
    /// by name — the control server dispatches incoming requests whose `type`
    /// field matches a registered command name to the plugin's handler.
    fn control_commands(&self, ctx: &Arc<ActivationContext>) -> Vec<ControlCommand> {
        let _ = ctx;
        vec![]
    }

    /// Collect all contributions (providers, scripts, control commands) in one call.
    ///
    /// The default implementation delegates to [`providers`](Self::providers),
    /// [`scripts`](Self::scripts), and [`control_commands`](Self::control_commands).
    /// Plugins should not need to override this.
    fn contributions(&self, ctx: &Arc<ActivationContext>) -> Result<PluginContributions> {
        Ok(PluginContributions {
            providers: self.providers(ctx)?,
            scripts: self.scripts(ctx)?,
            control_commands: self.control_commands(ctx),
        })
    }

    /// Default plugin configuration as a TOML table.
    ///
    /// Plugins that have configuration should override this to return their
    /// default config. The returned table is merged as the lowest-priority
    /// layer for `[plugin.<id>]` during config resolution.
    ///
    /// Returns `None` (default) if the plugin has no configuration.
    fn default_config(&self) -> Option<toml::Table> { None }

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
///
/// A function pointer (not a closure) because `linkme` distributed slices
/// require `Sync` statics with a fixed type. Each factory is called once
/// at mount time to produce the plugin instance.
pub type PluginFactory = fn() -> Box<dyn Plugin>;

/// Link-time distributed slice of plugin factories.
///
/// Each plugin crate contributes one entry via `#[distributed_slice(PLUGINS)]`.
/// At mount time, the registry iterates this slice to discover all plugins.
#[allow(unsafe_code)]
#[distributed_slice]
pub static PLUGINS: [PluginFactory];

/// Instantiate all linked plugins by calling their factory functions.
///
/// This is the standard entry point for obtaining plugin instances from the
/// [`PLUGINS`] distributed slice. Each factory is called exactly once.
pub fn instantiate() -> Vec<Box<dyn Plugin>> { PLUGINS.iter().map(|f| f()).collect() }

/// Implement [`Plugin::provider_graph`] by extracting constants from provider types.
///
/// Each provider type must define `PROVIDER_ID: ProviderId` and
/// `DEPENDENCIES: &[ProviderId]` as associated constants.
///
/// ```ignore
/// impl Plugin for MyPlugin {
///     provider_graph!(MyProvider, OtherProvider);
/// }
/// ```
#[macro_export]
macro_rules! provider_graph {
    ($($provider:ty),+ $(,)?) => {
        fn provider_graph(&self) -> &[($crate::router::ProviderId, &[$crate::router::ProviderId])] {
            const { &[$((<$provider>::PROVIDER_ID, <$provider>::DEPENDENCIES)),+] }
        }
    };
}

/// Implement [`Plugin::default_config`] and [`Plugin::resolved_config`] from a
/// [`PluginConfig`](crate::plugin::PluginConfig) type.
///
/// Place alongside [`provider_graph!`] at the top of a `Plugin` impl block.
///
/// ```ignore
/// impl Plugin for MyPlugin {
///     nyne::provider_graph!(MyProvider);
///     nyne::plugin_config!(Config);
///     fn id(&self) -> &str { "my-plugin" }
/// }
/// ```
#[macro_export]
macro_rules! plugin_config {
    ($config_ty:ty) => {
        fn default_config(&self) -> Option<toml::Table> {
            <$config_ty as $crate::plugin::PluginConfig>::default_table()
        }

        fn resolved_config(&self, config: &$crate::config::NyneConfig) -> Option<toml::Value> {
            <$config_ty as $crate::plugin::PluginConfig>::from_section(config.plugin.get(self.id())).to_value()
        }
    };
}
/// Register a plugin struct into the global [`PLUGINS`] distributed slice.
///
/// Emits the `#[distributed_slice(PLUGINS)]` + [`PluginFactory`] boilerplate
/// every plugin crate needs. Call once per plugin crate, passing a
/// zero-argument-constructible plugin struct (the typical unit-struct form).
///
/// ```ignore
/// struct GitPlugin;
///
/// impl nyne::plugin::Plugin for GitPlugin { /* ... */ }
///
/// nyne::register_plugin!(GitPlugin);
/// ```
#[macro_export]
macro_rules! register_plugin {
    ($Plugin:ident) => {
        /// Link-time registration of this crate's plugin into the global `PLUGINS` slice.
        #[allow(unsafe_code)]
        #[::linkme::distributed_slice($crate::plugin::PLUGINS)]
        static __NYNE_PLUGIN_FACTORY: $crate::plugin::PluginFactory = || ::std::boxed::Box::new($Plugin);
    };
}

/// Sort plugins by dependency order using [`Plugin::provider_graph`].
///
/// Builds a provider-ID → plugin-index mapping from [`Plugin::provider_graph`],
/// then topologically sorts plugins so dependencies activate first.
#[allow(clippy::indexing_slicing)] // graph node weights are always valid plugin indices
pub fn sort_by_deps(plugins: Vec<Box<dyn Plugin>>) -> Result<Vec<Box<dyn Plugin>>> {
    let topo = crate::topo::sort(
        &plugins,
        |p| p.provider_graph().iter().map(|&(id, _)| id).collect(),
        |p| {
            p.provider_graph()
                .iter()
                .flat_map(|(_, deps)| deps.iter().copied())
                .collect()
        },
    )
    .map_err(|c| color_eyre::eyre::eyre!("plugin dependency cycle involving {:?}", plugins[c.cycle_item].id()))?;

    let mut slots: Vec<_> = plugins.into_iter().map(Some).collect();
    Ok(topo.order.iter().filter_map(|&i| slots[i].take()).collect())
}
