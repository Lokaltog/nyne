//! Plugin registration and lifecycle implementation.

use std::sync::Arc;
use std::thread;

use color_eyre::eyre::Result;
use linkme::distributed_slice;
use nyne::config::NyneConfig;
use nyne::dispatch::activation::ActivationContext;
use nyne::plugin::{PLUGINS, Plugin, PluginFactory};
use nyne::provider::Provider;
use nyne::types::PassthroughProcesses;
use nyne_source::syntax::SyntaxRegistry;
use tracing::info;

use crate::config::LspConfig;
use crate::lsp::LspRegistry;
use crate::lsp::manager::LspManager;
use crate::lsp::path::LspPathResolver;
use crate::providers::workspace_search::WorkspaceSearchProvider;

/// Entry point for the LSP plugin, implementing the [`Plugin`] trait.
///
/// Unit struct that anchors plugin lifecycle methods. Registered into the
/// global plugin slice at link time via [`LSP_PLUGIN`].
struct LspPlugin;

/// Two-phase lifecycle for the LSP plugin.
///
/// During `activate`, the LSP registry and manager are constructed and the
/// manager is inserted into the `TypeMap` as `Arc<LspManager>`. LSP servers
/// are eagerly spawned on a background thread so they are warm before the
/// first workspace query arrives.
///
/// The `providers` phase creates the `WorkspaceSearchProvider` (and later,
/// `LspProvider` for per-symbol LSP nodes once wiring is complete).
impl Plugin for LspPlugin {
    fn id(&self) -> &'static str { "lsp" }

    fn activate(&self, ctx: &mut ActivationContext) -> Result<()> {
        let lsp_config = LspConfig::from_plugin_config(ctx.plugin_config("lsp"));
        let sandbox_env = ctx.config().sandbox.env.clone();
        let lsp_registry = LspRegistry::build_with_config(&lsp_config);

        // Contribute LSP server commands to the passthrough set so those
        // processes see only the real filesystem (not virtual content).
        ctx.insert(PassthroughProcesses::new(
            lsp_registry.server_commands().map(str::to_owned).collect(),
        ));

        let lsp = Arc::new(LspManager::new(
            lsp_registry,
            SyntaxRegistry::global(),
            lsp_config,
            Arc::clone(ctx.spawner()),
            sandbox_env,
            LspPathResolver::new(ctx.root().to_owned(), ctx.overlay_root().to_owned()),
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

        info!("lsp plugin activated");

        ctx.insert(lsp);

        Ok(())
    }

    fn providers(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<Arc<dyn Provider>>> {
        Ok(vec![Arc::new(WorkspaceSearchProvider::new(Arc::clone(ctx)))])
    }

    fn default_config(&self) -> Option<toml::Table> { toml::Table::try_from(LspConfig::default()).ok() }

    fn resolved_config(&self, config: &NyneConfig) -> Option<serde_json::Value> {
        serde_json::to_value(LspConfig::from_plugin_config(config.plugin.get("lsp"))).ok()
    }
}

/// Link-time registration of the LSP plugin into the global `PLUGINS` slice.
///
/// The binary's `main.rs` pulls in this crate with `use nyne_lsp as _;`,
/// which is enough for `linkme` to include this static in the final binary.
#[allow(unsafe_code)]
#[distributed_slice(PLUGINS)]
static LSP_PLUGIN: PluginFactory = || Box::new(LspPlugin);
