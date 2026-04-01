//! Plugin registration and lifecycle implementation.
pub mod config;

use std::sync::Arc;

use linkme::distributed_slice;
use nyne::config::PluginConfig;
use nyne::plugin::PluginFactory;
use nyne::prelude::*;
use nyne::router::Provider;
use nyne::templates::{HandleBuilder, TemplateHandle};
use nyne_companion::CompanionContextExt as _;
#[cfg(feature = "git")]
use nyne_git::GitContextExt as _;
use nyne_source::{SourceContextExt as _, SyntaxRegistry};
use parking_lot::RwLock;
use tracing::info;

use crate::context::TodoContextExt as _;
use crate::plugin::config::Config;
use crate::provider::scan::Scanner;
use crate::provider::{TodoProvider, TodoState, routes};

/// Entry point for the todo plugin.
struct TodoPlugin;

/// Plugin lifecycle for TODO/FIXME comment aggregation.
impl Plugin for TodoPlugin {
    nyne::provider_graph!(TodoProvider);

    nyne::plugin_config!(Config);

    fn id(&self) -> &'static str { "todo" }

    #[expect(clippy::expect_used, reason = "source plugin activation is a lifecycle invariant")]
    fn activate(&self, ctx: &mut ActivationContext) -> Result<()> {
        let config = Config::from_context(ctx, self.id());
        if !config.enabled {
            return Ok(());
        }

        #[cfg(feature = "git")]
        let repo = ctx.git_repo().cloned();

        let source_paths = ctx
            .source_paths()
            .cloned()
            .expect("SourcePaths missing — source plugin must activate first");

        let tags = config.tags.clone();
        let scanner = Scanner::new(&tags);
        let syntax = ctx.syntax_registry().cloned().unwrap_or_else(SyntaxRegistry::global);

        let mut b = HandleBuilder::new();
        let overview_key = b.register("todo/overview", include_str!("../provider/templates/overview.md.j2"));
        let tag_key = b.register("todo/tag", include_str!("../provider/templates/tag.md.j2"));
        let engine = b.finish();

        let state = Arc::new(TodoState {
            fs: Arc::clone(ctx.fs()),
            syntax,
            scanner,
            index: RwLock::new(None),
            overview_tmpl: TemplateHandle::new(&engine, overview_key),
            tag_tmpl: TemplateHandle::new(&engine, tag_key),
            tags,
            source_paths,
            vfs: config.vfs,
            #[cfg(feature = "git")]
            repo,
        });

        routes::register_companion_extensions(ctx.companion_extensions_mut(), &state);

        info!("todo plugin activated");
        ctx.insert(state);
        Ok(())
    }

    fn providers(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<Arc<dyn Provider>>> {
        let Some(state) = ctx.todo_state().cloned() else {
            return Ok(vec![]);
        };
        Ok(vec![Arc::new(TodoProvider { state })])
    }
}

/// Link-time registration of the todo plugin into the global `PLUGINS` slice.
#[allow(unsafe_code)]
#[distributed_slice(PLUGINS)]
static TODO_PLUGIN: PluginFactory = || Box::new(TodoPlugin);
