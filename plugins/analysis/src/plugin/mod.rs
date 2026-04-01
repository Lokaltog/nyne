//! Plugin registration and lifecycle implementation.
pub mod config;

use std::sync::Arc;

use color_eyre::eyre::Result;
use linkme::distributed_slice;
use nyne::ActivationContext;
use nyne::config::PluginConfig;
use nyne::plugin::{PLUGINS, Plugin, PluginFactory};
use nyne::router::{NamedNode, Provider, Request, RouteCtx};
use nyne::templates::{HandleBuilder, TemplateGlobals, TemplateHandle};
use nyne_companion::CompanionRequest as _;
use nyne_source::{FragmentResolver, SourceContextExt, SyntaxRegistry, find_fragment};
use tracing::info;

use crate::engine::Engine;
use crate::plugin::config::Config;
use crate::provider;

/// Entry point for the analysis plugin.
struct AnalysisPlugin;

/// Plugin lifecycle for the analysis engine.
///
/// During activation, reads `[plugin.analysis]` config, builds a filtered
/// [`Engine`] respecting enabled/disabled rules, and inserts it into
/// the `AnyMap` as `Arc<Engine>`. The provider phase creates an
/// [`AnalysisProvider`] that contributes `ANALYSIS.md` to symbol directories.
impl Plugin for AnalysisPlugin {
    nyne::plugin_config!(Config);

    fn id(&self) -> &'static str { "analysis" }

    #[allow(clippy::excessive_nesting)]
    fn activate(&self, ctx: &mut ActivationContext) -> Result<()> {
        let config = Config::from_context(ctx, self.id());
        let engine = Arc::new(Engine::build_filtered(&config));

        info!(
            enabled = config.enabled,
            rules = ?config.rules,
            "analysis plugin activated",
        );

        // Build template handle for ANALYSIS.md.
        let file_analysis = config.vfs.file.analysis.clone();
        let mut builder = HandleBuilder::new();
        config.vfs.register_globals(builder.engine_mut());
        let key = builder.register("analysis/content", include_str!("../templates/analysis.md.j2"));
        let tmpl = TemplateHandle::new(&builder.finish(), key);

        // Register into SourceExtensions so ANALYSIS.md appears alongside
        // body, signature, etc. inside fragment directories.
        let syntax = ctx.syntax_registry().cloned().unwrap_or_else(SyntaxRegistry::global);
        let decomposition = ctx.decomposition_cache().cloned();

        if let Some(decomposition) = decomposition {
            let exts = ctx.source_extensions_mut();
            exts.fragment_path.scoped("analysis", |ext| {
                let syntax = Arc::clone(&syntax);
                let decomposition = decomposition.clone();
                let engine = Arc::clone(&engine);
                let tmpl = tmpl.clone();
                let file_analysis = file_analysis.clone();
                ext.content(move |ctx: &RouteCtx, req: &Request| -> Option<NamedNode> {
                    let sf = req.source_file()?;
                    let path_param = ctx.param("path")?;
                    let segments: Vec<String> = path_param.split('/').map(String::from).collect();

                    syntax.decomposer_for(&sf)?;
                    let shared = decomposition.get(&sf).ok()?;
                    find_fragment(&shared.decomposed, &segments)?;

                    let resolver = FragmentResolver::new(decomposition.clone(), sf);
                    Some(tmpl.named_node(&file_analysis, provider::Content {
                        resolver,
                        engine: Arc::clone(&engine),
                    }))
                });
            });
        }

        ctx.insert(engine);
        Ok(())
    }

    fn providers(&self, _ctx: &Arc<ActivationContext>) -> Result<Vec<Arc<dyn Provider>>> {
        // All content is contributed via SourceExtensions.fragment_path
        // registered in activate(). No provider needed.
        Ok(vec![])
    }
}

/// Link-time registration of the analysis plugin into the global `PLUGINS` slice.
#[allow(unsafe_code)]
#[distributed_slice(PLUGINS)]
static ANALYSIS_PLUGIN: PluginFactory = || Box::new(AnalysisPlugin);
