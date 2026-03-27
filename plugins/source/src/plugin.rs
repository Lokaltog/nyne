use linkme::distributed_slice;
use nyne::config::NyneConfig;
use nyne::plugin::PluginFactory;
use nyne::prelude::*;
use tracing::info;

use crate::config::Config;
use crate::services::Services;
use crate::syntax::SyntaxRegistry;
use crate::syntax::decomposed::DecompositionCache;

/// Entry point for the source plugin, implementing the [`Plugin`] trait.
///
/// This is a unit struct that serves as the anchor for plugin lifecycle
/// methods. It is instantiated by [`SOURCE_PLUGIN`] and registered into the
/// global plugin slice at link time. All mutable state lives in
/// [`Services`], which is inserted into the `TypeMap` during activation.
pub struct SourcePlugin;

/// Two-phase lifecycle for the source plugin.
///
/// During `activate`, all heavyweight services (syntax registry,
/// decomposition cache, analysis engine) are constructed and bundled into a
/// single [`Services`] inserted into the `TypeMap`. The `providers`
/// phase then creates provider instances that read from that bundle.
impl Plugin for SourcePlugin {
    /// Returns the unique identifier for this plugin (`"source"`).
    fn id(&self) -> &'static str { "source" }

    /// Constructs and registers all source services into the activation context.
    ///
    /// This is the "heavy" phase: it builds the syntax registry and
    /// creates the decomposition cache, then assembles the
    /// [`Services`] bundle inserted into the `TypeMap`.
    fn activate(&self, ctx: &mut ActivationContext) -> Result<()> {
        let syntax = SyntaxRegistry::global();

        let source_config = Config::from_plugin_config(ctx.plugin_config("source"));

        let decomposition = DecompositionCache::new(Arc::clone(ctx.real_fs()), Arc::clone(&syntax));

        info!(languages = syntax.extensions().len(), "source plugin activated",);

        ctx.insert(Services {
            syntax,
            decomposition,
            config: source_config,
        });

        Ok(())
    }

    /// Returns the fully resolved source plugin configuration as JSON.
    ///
    /// Re-derives [`Config`] from the raw config map so the output
    /// reflects all defaults, not just what the user explicitly set. Used
    /// by `nyne config` to show the effective configuration.
    fn resolved_config(&self, config: &NyneConfig) -> Option<serde_json::Value> {
        let resolved = Config::from_plugin_config(config.plugin.get("source"));
        serde_json::to_value(&resolved).ok()
    }

    /// Returns the default configuration for the source plugin as a TOML table.
    ///
    /// Used by the config system to populate missing fields before
    /// [`resolved_config`] is called.
    fn default_config(&self) -> Option<toml::Table> { toml::Table::try_from(Config::default()).ok() }

    /// Instantiates all source plugin providers from the activated context.
    ///
    /// The core set is always present: syntax decomposition and
    /// batch edit staging.
    /// When the `git-symbols` feature is enabled **and** the git plugin
    /// has successfully opened a repository, a `GitSymbolsProvider` is
    /// appended for per-symbol blame and history.
    fn providers(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<Arc<dyn Provider>>> {
        use crate::providers::batch::BatchEditProvider;
        use crate::providers::syntax::SyntaxProvider;

        #[cfg_attr(not(feature = "git-symbols"), allow(unused_mut))]
        let mut providers: Vec<Arc<dyn Provider>> = vec![
            Arc::new(SyntaxProvider::new(Arc::clone(ctx))),
            Arc::new(BatchEditProvider::new(Arc::clone(ctx))),
        ];

        // Symbol-scoped git features (per-symbol blame/history) — only
        // when the git plugin has opened a repo.
        #[cfg(feature = "git-symbols")]
        if ctx.get::<Arc<nyne_git::Repo>>().is_some() {
            use crate::providers::git_symbols_companion::GitSymbolsProvider;
            providers.push(Arc::new(GitSymbolsProvider::new(Arc::clone(ctx))));
        }

        Ok(providers)
    }
}

/// Link-time registration of the source plugin into the global `PLUGINS` slice.
///
/// The binary's `main.rs` pulls in this crate with `use nyne_source as _;`,
/// which is enough for `linkme` to include this static in the final binary.
/// At startup, the framework iterates `PLUGINS` and calls each factory to
/// obtain a `Box<dyn Plugin>`.
#[allow(unsafe_code)]
#[distributed_slice(PLUGINS)]
static SOURCE_PLUGIN: PluginFactory = || Box::new(SourcePlugin);
