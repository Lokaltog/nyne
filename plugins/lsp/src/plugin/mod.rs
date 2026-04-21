//! Plugin registration and lifecycle implementation.
pub mod config;

use std::sync::Arc;
use std::thread;

use color_eyre::eyre::Result;
use nyne::ActivationContext;
use nyne::plugin::{Plugin, PluginConfig};
use nyne::router::Provider;
use nyne_companion::CompanionContextExt;
use nyne_source::{DecompositionCache, SourceContextExt, SourcePaths, SyntaxRegistry};
use nyne_visibility::PassthroughProcesses;
use tracing::info;

use crate::context::LspContextExt;
use crate::plugin::config::Config;
use crate::provider::state::build_handles;
use crate::provider::{LspProvider, LspRenameProvider, LspState, routes};
use crate::session::Registry;
use crate::session::manager::Manager;
use crate::session::path::PathResolver;

/// Entry point for the LSP plugin, implementing the [`Plugin`] trait.
///
/// Registered into the global plugin slice at link time via [`LSP_PLUGIN`].
struct LspPlugin;

/// Two-phase lifecycle for the LSP plugin.
///
/// During `activate`, the LSP registry and manager are constructed and the
/// manager is inserted into the `AnyMap` as `Arc<Manager>`. LSP servers
/// are eagerly spawned on a background thread so they are warm before the
/// first workspace query arrives.
///
/// The `providers` phase creates `LspRenameProvider` (file rename
/// coordination) and `LspProvider` (per-symbol LSP nodes). Workspace
/// symbol search is registered as a mount-wide companion extension.
impl Plugin for LspPlugin {
    nyne::provider_graph!(LspRenameProvider, LspProvider);

    nyne::plugin_config!(Config);

    fn id(&self) -> &'static str { "lsp" }

    fn activate(&self, ctx: &mut ActivationContext) -> Result<()> {
        let lsp_config = ctx.plugin_config::<Config>(self.id());
        let sandbox_env = ctx.config().sandbox.env.clone();
        let lsp_registry = Registry::build_with_config(&lsp_config);

        // Contribute LSP server commands to the passthrough set so those
        // processes see only the real filesystem (not virtual content).
        ctx.insert(PassthroughProcesses::new(
            lsp_registry.server_commands().into_iter().map(str::to_owned).collect(),
        ));

        let lsp = Arc::new(Manager::new(
            lsp_registry,
            SyntaxRegistry::global(),
            lsp_config.clone(),
            Arc::clone(ctx.spawner()),
            sandbox_env,
            PathResolver::new(ctx.root().to_owned(), ctx.source_root().to_owned()),
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

        // Build shared LSP state — services from source plugin.
        // Source activates before LSP (guaranteed by provider_graph ordering).
        let syntax = Arc::clone(ctx.require_service::<Arc<SyntaxRegistry>>()?);
        let decomposition = ctx.require_service::<DecompositionCache>()?.clone();
        let source_paths = Arc::clone(ctx.require_service::<Arc<SourcePaths>>()?);
        let state = Arc::new(LspState {
            lsp: Arc::clone(&lsp),
            syntax,
            decomposition,
            fs: Arc::clone(ctx.fs()),
            handles: build_handles(&lsp_config.vfs),
            vfs: lsp_config.vfs,
            source_paths,
        });

        // Register extension callbacks into companion and source extension points.
        routes::register_companion_extensions(ctx.companion_extensions_mut(), &state);
        routes::register_mount_extensions(ctx.companion_extensions_mut(), &state);
        routes::register_source_extensions(ctx.source_extensions_mut(), &state);

        ctx.insert(Arc::clone(&state));
        ctx.insert(lsp);

        info!("lsp plugin activated");

        Ok(())
    }

    #[expect(
        clippy::expect_used,
        reason = "activate() inserts all required services (ordering guaranteed by provider_graph)"
    )]
    fn providers(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<Arc<dyn Provider>>> {
        let state = Arc::clone(ctx.lsp_state().expect("LspState missing"));

        Ok(vec![
            Arc::new(LspRenameProvider {
                state: Arc::clone(&state),
            }),
            Arc::new(LspProvider { state }),
        ])
    }
}

nyne::register_plugin!(LspPlugin);
