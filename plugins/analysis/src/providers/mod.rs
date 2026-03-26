mod analysis;

use std::sync::Arc;

use nyne::dispatch::activation::ActivationContext;
use nyne::dispatch::context::RequestContext;
use nyne::dispatch::routing::ctx::RouteCtx;
use nyne::dispatch::routing::tree::RouteTree;
use nyne::provider::{Nodes, Provider, ProviderId};
use nyne::templates::TemplateHandle;
use nyne::types::path_conventions::split_companion_path;
use nyne::{companion_children, source_file};
use nyne_source::providers::fragment_resolver::FragmentResolver;
use nyne_source::providers::names::handle_builder;
use nyne_source::services::SourceServices;
use nyne_source::syntax::find_fragment;

const FILE_ANALYSIS: &str = "ANALYSIS.md";

pub struct AnalysisProvider {
    ctx: Arc<ActivationContext>,
    analysis: TemplateHandle,
    routes: RouteTree<Self>,
}

impl AnalysisProvider {
    const PROVIDER_ID: ProviderId = ProviderId::new("analysis");

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
        let node = self.analysis.node(FILE_ANALYSIS, analysis::AnalysisContent {
            resolver,
            activation: Arc::clone(&self.ctx),
        });
        Ok(Some(vec![node]))
    }

    fn services(&self) -> &SourceServices { SourceServices::get(&self.ctx) }
}

impl Provider for AnalysisProvider {
    fn id(&self) -> ProviderId { Self::PROVIDER_ID }

    fn children(self: Arc<Self>, ctx: &RequestContext<'_>) -> Nodes {
        let Some(split) = split_companion_path(ctx.path) else {
            return Ok(None);
        };
        companion_children(&self.routes, &self, ctx, &split)
    }
}
