//! Provider registration for the analysis plugin.
//!
//! Re-exports the [`AnalysisProvider`] which contributes `ANALYSIS.md` nodes
//! to symbol fragment directories in the VFS.

mod analysis;

use nyne::dispatch::routing::ctx::RouteCtx;
use nyne::dispatch::routing::tree::RouteTree;
use nyne::prelude::*;
use nyne::templates::TemplateHandle;
use nyne::types::path_conventions::split_companion_path;
use nyne::{companion_children, source_file};
use nyne_source::providers::fragment_resolver::FragmentResolver;
use nyne_source::providers::well_known::handle_builder;
use nyne_source::services::Services;
use nyne_source::syntax::find_fragment;

const FILE_ANALYSIS: &str = "ANALYSIS.md";

pub struct AnalysisProvider {
    ctx: Arc<ActivationContext>,
    analysis: TemplateHandle,
    routes: RouteTree<Self>,
}

/// Construction, routing, and fragment resolution for [`AnalysisProvider`].
impl AnalysisProvider {
    /// Provider identifier registered with the dispatch layer.
    const PROVIDER_ID: ProviderId = ProviderId::new("analysis");

    /// Create a new analysis provider, compiling the Jinja template and
    /// building the route tree for `symbols/{..path}@/ANALYSIS.md`.
    pub fn new(ctx: Arc<ActivationContext>) -> Self {
        let mut builder = handle_builder();
        nyne::register_globals!(builder.engine_mut(), FILE_ANALYSIS,);
        let analysis_key = builder.register("analysis/content", include_str!("../templates/analysis.md.j2"));
        let engine = builder.finish();
        let analysis = TemplateHandle::new(&engine, analysis_key);

        Self {
            ctx,
            analysis,
            routes: nyne_macros::routes!(Self, {
                "symbols" {
                    "{..path}@" => children_fragment,
                }
            }),
        }
    }

    /// Contribute an `ANALYSIS.md` node to a symbol fragment directory.
    ///
    /// Returns `None` if the file has no decomposer or the fragment path
    /// doesn't resolve, so analysis nodes only appear for supported languages.
    fn children_fragment(&self, ctx: &RouteCtx<'_>) -> Nodes {
        let sf = source_file(ctx)?;
        let path = ctx.params("path");

        let services = self.services();
        if services.syntax.decomposer_for(&sf).is_none() {
            return Ok(None);
        }
        let shared = services.decomposition.get(&sf)?;
        if find_fragment(&shared.decomposed, path).is_none() {
            return Ok(None);
        }

        let resolver = FragmentResolver::new(services.decomposition.clone(), sf);
        let node = self.analysis.node(FILE_ANALYSIS, analysis::Content {
            resolver,
            activation: Arc::clone(&self.ctx),
        });
        Ok(Some(vec![node]))
    }

    /// Shorthand to retrieve [`Services`] from the activation context.
    fn services(&self) -> &Services { Services::get(&self.ctx) }
}

/// [`Provider`] implementation that routes companion-path requests to the
/// analysis route tree, contributing `ANALYSIS.md` to symbol directories.
impl Provider for AnalysisProvider {
    fn id(&self) -> ProviderId { Self::PROVIDER_ID }

    fn children(self: Arc<Self>, ctx: &RequestContext<'_>) -> Nodes {
        let Some(split) = split_companion_path(ctx.path) else {
            return Ok(None);
        };
        companion_children(&self.routes, &self, ctx, &split)
    }
}
