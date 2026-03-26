use std::sync::Arc;
use std::thread;

use color_eyre::eyre::Result;
use linkme::distributed_slice;
use nyne::config::NyneConfig;
use nyne::dispatch::activation::ActivationContext;
use nyne::dispatch::script::ScriptEntry;
use nyne::plugin::{PLUGINS, Plugin, PluginFactory};
use nyne::provider::Provider;
use nyne::types::PassthroughProcesses;
use tracing::info;

use crate::config::CodingConfig;
use crate::lsp::LspRegistry;
use crate::lsp::manager::LspManager;
use crate::lsp::path::LspPathResolver;
use crate::providers::claude;
use crate::services::CodingServices;
use crate::syntax::SyntaxRegistry;
use crate::syntax::analysis::AnalysisEngine;
use crate::syntax::decomposed::DecompositionCache;

/// Entry point for the coding plugin, implementing the [`Plugin`] trait.
///
/// This is a unit struct that serves as the anchor for plugin lifecycle
/// methods. It is instantiated by [`CODING_PLUGIN`] and registered into the
/// global plugin slice at link time. All mutable state lives in
/// [`CodingServices`], which is inserted into the `TypeMap` during activation.
pub struct CodingPlugin;

/// Two-phase lifecycle for the coding plugin.
///
/// During `activate`, all heavyweight services (syntax registry, LSP manager,
/// decomposition cache, analysis engine) are constructed and bundled into a
/// single [`CodingServices`] inserted into the `TypeMap`. The `providers`
/// phase then creates provider instances that read from that bundle.
///
/// LSP servers are eagerly spawned on a background thread during activation
/// so they are warm before the first workspace query arrives.
impl Plugin for CodingPlugin {
    fn id(&self) -> &'static str { "coding" }

    /// Constructs and registers all coding services into the activation context.
    ///
    /// This is the "heavy" phase: it builds the LSP registry from config,
    /// contributes LSP server commands to the passthrough set (so those
    /// processes bypass the FUSE overlay and see the real filesystem),
    /// creates the [`LspManager`] with eager background spawning, and
    /// assembles the [`CodingServices`] bundle inserted into the `TypeMap`.
    fn activate(&self, ctx: &mut ActivationContext) -> Result<()> {
        let syntax = SyntaxRegistry::global();

        let coding_config = CodingConfig::from_plugin_config(ctx.plugin_config("coding"));
        let sandbox_env = ctx.config().sandbox.env.clone();
        let lsp_registry = LspRegistry::build_with_config(&coding_config.lsp);
        let lsp_config = coding_config.lsp.clone();

        // Contribute LSP server commands to the passthrough set so those
        // processes see only the real filesystem (not virtual content).
        ctx.insert(PassthroughProcesses::new(
            lsp_registry.server_commands().map(str::to_owned).collect(),
        ));

        let lsp_path_resolver = LspPathResolver::new(ctx.root().to_owned(), ctx.overlay_root().to_owned());
        let lsp = Arc::new(LspManager::new(
            lsp_registry,
            Arc::clone(&syntax),
            lsp_config,
            Arc::clone(ctx.spawner()),
            sandbox_env,
            lsp_path_resolver,
        ));

        // Eagerly spawn LSP servers in the background so they're warm
        // by the time workspace-wide queries (e.g. symbol search) arrive.
        {
            let lsp = Arc::clone(&lsp);
            thread::Builder::new()
                .name("lsp-eager-spawn".into())
                .spawn(move || lsp.spawn_all_applicable())
                .ok();
        }

        let decomposition = DecompositionCache::new(Arc::clone(ctx.real_fs()), Arc::clone(&syntax));
        let analysis = Arc::new(AnalysisEngine::build_filtered(&coding_config.analysis));

        info!(
            languages = syntax.extensions().len(),
            analysis_enabled = coding_config.analysis.enabled,
            analysis_rules = ?coding_config.analysis.rules,
            "coding plugin activated",
        );

        ctx.insert(CodingServices {
            syntax,
            lsp,
            decomposition,
            analysis,
            config: coding_config,
        });

        Ok(())
    }

    /// Returns Claude hook script entries that should be installed in `.claude/`.
    ///
    /// Which hooks are emitted depends on the per-hook toggles in
    /// `[plugin.coding.claude.hooks]`. When the Claude master toggle is off,
    /// this returns an empty list and no hooks are installed.
    fn scripts(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<ScriptEntry>> {
        Ok(claude::script_entries(&CodingServices::get(ctx).config))
    }

    /// Returns the fully resolved coding plugin configuration as JSON.
    ///
    /// Re-derives [`CodingConfig`] from the raw config map so the output
    /// reflects all defaults, not just what the user explicitly set. Used
    /// by `nyne config` to show the effective configuration.
    fn resolved_config(&self, config: &NyneConfig) -> Option<serde_json::Value> {
        let resolved = CodingConfig::from_plugin_config(config.plugin.get("coding"));
        serde_json::to_value(&resolved).ok()
    }

    fn default_config(&self) -> Option<toml::Table> { toml::Table::try_from(CodingConfig::default()).ok() }

    /// Instantiates all coding plugin providers from the activated context.
    ///
    /// The core set is always present: syntax decomposition, Claude hooks,
    /// todo tracking, batch edit staging, and workspace symbol search.
    /// When the `git-symbols` feature is enabled **and** the git plugin
    /// has successfully opened a repository, a `GitSymbolsProvider` is
    /// appended for per-symbol blame and history.
    fn providers(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<Arc<dyn Provider>>> {
        use crate::providers::batch::BatchEditProvider;
        use crate::providers::claude::ClaudeProvider;
        use crate::providers::syntax::SyntaxProvider;
        use crate::providers::todo::TodoProvider;
        use crate::providers::workspace_search::WorkspaceSearchProvider;

        #[cfg_attr(not(feature = "git-symbols"), allow(unused_mut))]
        let mut providers: Vec<Arc<dyn Provider>> = vec![
            Arc::new(SyntaxProvider::new(Arc::clone(ctx))),
            Arc::new(ClaudeProvider::new(Arc::clone(ctx))),
            Arc::new(TodoProvider::new(Arc::clone(ctx))),
            Arc::new(BatchEditProvider::new(Arc::clone(ctx))),
            Arc::new(WorkspaceSearchProvider::new(Arc::clone(ctx))),
        ];

        // Symbol-scoped git features (per-symbol blame/history) — only
        // when the git plugin has opened a repo.
        #[cfg(feature = "git-symbols")]
        if ctx.get::<Arc<nyne_git::GitRepo>>().is_some() {
            use crate::providers::git_symbols_companion::GitSymbolsProvider;
            providers.push(Arc::new(GitSymbolsProvider::new(Arc::clone(ctx))));
        }

        Ok(providers)
    }
}

/// Link-time registration of the coding plugin into the global `PLUGINS` slice.
///
/// The binary's `main.rs` pulls in this crate with `use nyne_coding as _;`,
/// which is enough for `linkme` to include this static in the final binary.
/// At startup, the framework iterates `PLUGINS` and calls each factory to
/// obtain a `Box<dyn Plugin>`.
#[allow(unsafe_code)]
#[distributed_slice(PLUGINS)]
static CODING_PLUGIN: PluginFactory = || Box::new(CodingPlugin);
