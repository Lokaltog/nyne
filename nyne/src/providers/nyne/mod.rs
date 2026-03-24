//! Nyne provider — root-level meta files (GUIDE, STATUS).

use std::time::Instant;

use color_eyre::eyre::Result;
use nyne_macros::routes;
use serde::Serialize;

use super::names::{self, COMPANION_SUFFIX, FILE_GUIDE, FILE_MOUNT_STATUS};
use super::prelude::*;
use crate::dispatch::routing::ctx::RouteCtx;
use crate::dispatch::routing::tree::RouteTree;
use crate::plugin::PLUGINS;
use crate::templates::{TemplateEngine, TemplateHandle, TemplateView, serialize_view};
use crate::types::ExtensionCounts;

/// Provider for root-level nyne meta files (`@/GUIDE.md`, `@/STATUS.md`).
pub(super) struct NyneProvider {
    guide: TemplateHandle,
    status: TemplateHandle,
    guide_view: GuideView,
    status_view: StatusView,
    routes: RouteTree<Self>,
}

/// Construction and route handlers for the nyne meta provider.
impl NyneProvider {
    /// Creates a new nyne provider with guide and status templates.
    pub(super) fn new(ctx: &Arc<ActivationContext>) -> Self {
        let source_dir = ctx.root().display().to_string();
        let empty = ExtensionCounts::default();
        let ext_counts = ctx.get::<ExtensionCounts>().unwrap_or(&empty);
        let languages = super::util::languages_display(&ext_counts.0);
        let ext = super::util::dominant_ext(&ext_counts.0).to_owned();

        let mut b = names::handle_builder();
        let guide_key = b.register("nyne/guide", include_str!("templates/guide.md.j2"));
        let status_key = b.register("nyne/status", include_str!("templates/status.md.j2"));
        let engine = b.finish();
        let guide = TemplateHandle::new(&engine, guide_key);
        let status = TemplateHandle::new(&engine, status_key);

        let guide_view = GuideView {
            source_dir,
            languages,
            ext,
        };
        let status_view = StatusView {
            ctx: Arc::clone(ctx),
            start_time: Instant::now(),
        };

        let routes = routes!(Self, {
            children(children_root), lookup(lookup_root),
            no_emit "@" => children_at_root,
        });

        Self {
            guide,
            status,
            guide_view,
            status_view,
            routes,
        }
    }

    /// Emit `@` as a hidden child of the root directory so it appears in
    /// readdir when `ProcessVisibility::All` is active.
    #[expect(clippy::unused_self, reason = "route handler called as instance method")]
    #[expect(clippy::unnecessary_wraps, reason = "route tree requires Nodes return type")]
    fn children_root(&self, ctx: &RouteCtx<'_>) -> Nodes {
        if ctx.path.is_root() {
            Ok(Some(vec![super::companion_dir(COMPANION_SUFFIX)]))
        } else {
            Ok(None)
        }
    }

    /// Emit `@` directory at mount root (lookup fallback).
    #[expect(clippy::unused_self, reason = "route handler called as instance method")]
    fn lookup_root(&self, ctx: &RouteCtx<'_>, name: &str) -> Option<VirtualNode> {
        // Ensure @/ directory exists at mount root even if the git provider is
        // inactive (no git repo). The git provider also creates this; the
        // router deduplicates identical directory nodes.
        if ctx.path.is_root() && name == COMPANION_SUFFIX {
            return Some(super::companion_dir(COMPANION_SUFFIX));
        }
        None
    }

    /// At `@/` level — emit GUIDE.md and STATUS.md.
    fn children_at_root(&self, _ctx: &RouteCtx<'_>) -> Vec<VirtualNode> {
        vec![
            self.guide.node(FILE_GUIDE, serialize_view(self.guide_view.clone())),
            self.status
                .node(FILE_MOUNT_STATUS, StatusView {
                    ctx: Arc::clone(&self.status_view.ctx),
                    start_time: self.status_view.start_time,
                })
                .with_cache_policy(CachePolicy::Never),
        ]
    }
}

/// Provider implementation for root-level nyne meta files.
impl Provider for NyneProvider {
    /// Returns the nyne provider identifier.
    fn id(&self) -> ProviderId { Self::PROVIDER_ID }

    /// Dispatches children resolution through the route tree.
    fn children(self: Arc<Self>, ctx: &RequestContext<'_>) -> Nodes { self.routes.children(&self, ctx) }

    /// Dispatches lookup resolution through the route tree.
    fn lookup(self: Arc<Self>, ctx: &RequestContext<'_>, name: &str) -> Node { self.routes.lookup(&self, ctx, name) }
}

#[derive(Serialize, Clone)]
/// Guide view — project info and language distribution.
struct GuideView {
    source_dir: String,
    languages: String,
    ext: String,
}

/// Status view — mount runtime and project stats.
///
/// Computes the active provider list at render time from the
/// `ActivationContext`, so it always reflects the actual activation
/// state without duplicating each provider's `should_activate()` logic.
struct StatusView {
    ctx: Arc<ActivationContext>,
    start_time: Instant,
}

/// Renders the mount status template with live runtime data.
impl TemplateView for StatusView {
    /// Renders uptime, active providers, and project info into the status template.
    fn render(&self, engine: &TemplateEngine, template: &str) -> Result<Vec<u8>> {
        let uptime = humantime::format_duration(self.start_time.elapsed()).to_string();
        let providers: Vec<&str> = PLUGINS
            .iter()
            .filter_map(|factory| factory().providers(&self.ctx).ok())
            .flatten()
            .filter(|p| p.should_activate(&self.ctx))
            .map(|p| p.id().as_str())
            .collect();
        let view = minijinja::context! {
            source_dir => self.ctx.root().display().to_string(),
            languages => {
                let empty = ExtensionCounts::default();
                let ext_counts = self.ctx.get::<ExtensionCounts>().unwrap_or(&empty);
                super::util::languages_display(&ext_counts.0)
            },
            providers,
            uptime,
        };
        Ok(engine.render_bytes(template, &view))
    }
}

/// Associated constants for the nyne meta provider.
impl NyneProvider {
    /// The provider identifier for the nyne meta provider.
    const PROVIDER_ID: ProviderId = ProviderId::new("nyne");
}
