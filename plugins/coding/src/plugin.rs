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
use crate::syntax::SyntaxRegistry;
use crate::syntax::analysis::AnalysisEngine;
use crate::syntax::decomposed::DecompositionCache;

pub struct CodingPlugin;

impl Plugin for CodingPlugin {
    fn id(&self) -> &'static str { "coding" }

    fn activate(&self, ctx: &mut ActivationContext) -> Result<()> {
        let syntax = SyntaxRegistry::global();
        ctx.insert(Arc::clone(&syntax));

        let (coding_config, lsp_registry, lsp_config, sandbox_env) = {
            let config = ctx.config();
            (
                CodingConfig::from_plugin_table(&config.plugin),
                LspRegistry::build_with_config(&config.lsp),
                config.lsp.clone(),
                config.sandbox.env.clone(),
            )
        };

        // Contribute LSP server commands to the passthrough set so those
        // processes see only the real filesystem (not virtual content).
        let lsp_commands: Vec<String> = lsp_registry.server_commands().map(str::to_owned).collect();
        ctx.insert(PassthroughProcesses(lsp_commands));

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
        let lsp_bg = Arc::clone(&lsp);
        thread::Builder::new()
            .name("lsp-eager-spawn".into())
            .spawn(move || lsp_bg.spawn_all_applicable())
            .ok();

        ctx.insert(lsp);

        let decomp_cache = DecompositionCache::new(Arc::clone(ctx.real_fs()), Arc::clone(&syntax));
        ctx.insert(decomp_cache);

        let analysis = Arc::new(AnalysisEngine::build_filtered(&coding_config.analysis));
        ctx.insert(analysis);

        info!(
            languages = syntax.extensions().len(),
            analysis_enabled = coding_config.analysis.enabled,
            analysis_rules = ?coding_config.analysis.rules,
            "coding plugin activated",
        );

        ctx.insert(coding_config);

        Ok(())
    }

    fn scripts(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<ScriptEntry>> {
        let coding_config = ctx
            .get::<CodingConfig>()
            .ok_or_else(|| color_eyre::eyre::eyre!("coding plugin not activated"))?;
        Ok(claude::script_entries(coding_config))
    }

    fn resolved_config(&self, config: &NyneConfig) -> Option<toml::Value> {
        let resolved = CodingConfig::from_plugin_table(&config.plugin);
        toml::Value::try_from(&resolved).ok()
    }

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

#[allow(unsafe_code)]
#[distributed_slice(PLUGINS)]
static CODING_PLUGIN: PluginFactory = || Box::new(CodingPlugin);
