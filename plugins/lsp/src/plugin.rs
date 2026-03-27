//! Plugin registration and lifecycle implementation.

use std::path::Path;
use std::sync::OnceLock;
use std::thread;

use color_eyre::eyre;
use linkme::distributed_slice;
use nyne::config::NyneConfig;
use nyne::plugin::PluginFactory;
use nyne::prelude::*;
use nyne::types::PassthroughProcesses;
use nyne_source::providers::syntax::{FileRenameHook, FragmentNodeHook};
use nyne_source::syntax::SyntaxRegistry;
use tracing::info;

use crate::config::Config;
use crate::lsp::Registry;
use crate::lsp::handle::Handle;
use crate::lsp::manager::Manager;
use crate::lsp::path::PathResolver;
use crate::providers::content::rename::SymbolRename;
use crate::providers::provider::LspProvider;
use crate::providers::workspace_search::WorkspaceSearchProvider;

/// Entry point for the LSP plugin, implementing the [`Plugin`] trait.
///
/// Registered into the global plugin slice at link time via [`LSP_PLUGIN`].
/// Caches the resolved [`Config`] from `activate` so `resolved_config`
/// can return it without re-parsing.
struct LspPlugin {
    resolved: OnceLock<Config>,
}

/// Two-phase lifecycle for the LSP plugin.
///
/// During `activate`, the LSP registry and manager are constructed and the
/// manager is inserted into the `TypeMap` as `Arc<LspManager>`. LSP servers
/// are eagerly spawned on a background thread so they are warm before the
/// first workspace query arrives.
///
/// The `providers` phase creates `LspProvider` (per-symbol LSP nodes) and
/// `WorkspaceSearchProvider` (workspace symbol search).
impl Plugin for LspPlugin {
    fn id(&self) -> &'static str { "lsp" }

    fn activate(&self, ctx: &mut ActivationContext) -> Result<()> {
        let lsp_config = Config::from_plugin_config(ctx.plugin_config("lsp"));
        let _ = self.resolved.set(lsp_config.clone());
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
            lsp_config,
            Arc::clone(ctx.spawner()),
            sandbox_env,
            PathResolver::new(ctx.root().to_owned(), ctx.overlay_root().to_owned()),
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

        // Insert the FileRenameHook so SyntaxProvider can coordinate
        // file renames with the LSP server.
        let rename_hook: Arc<dyn FileRenameHook> = Arc::new(LspFileRenameHook(Arc::clone(&lsp)));
        ctx.insert(rename_hook);

        // Insert the FragmentNodeHook so SyntaxProvider can attach
        // LSP Renameable to fragment directory nodes at construction time.
        let fragment_hook: Arc<dyn FragmentNodeHook> = Arc::new(LspFragmentNodeHook);
        ctx.insert(fragment_hook);

        info!("lsp plugin activated");

        ctx.insert(lsp);

        Ok(())
    }

    fn providers(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<Arc<dyn Provider>>> {
        Ok(vec![
            Arc::new(LspProvider::new(Arc::clone(ctx))),
            Arc::new(WorkspaceSearchProvider::new(Arc::clone(ctx))),
        ])
    }

    fn default_config(&self) -> Option<toml::Table> { toml::Table::try_from(Config::default()).ok() }

    /// Return the resolved LSP config as JSON for `nyne config` output.
    fn resolved_config(&self, config: &NyneConfig) -> Option<serde_json::Value> {
        let cfg = self
            .resolved
            .get()
            .cloned()
            .unwrap_or_else(|| Config::from_plugin_config(config.plugin.get("lsp")));
        serde_json::to_value(cfg).ok()
    }
}

/// File rename hook delegating to [`Manager`].
///
/// Inserted into the `TypeMap` during `activate()` so that
/// `SyntaxProvider` can coordinate file renames with the LSP server.
struct LspFileRenameHook(Arc<Manager>);

/// Delegates file rename lifecycle events to [`Manager`].
impl FileRenameHook for LspFileRenameHook {
    /// Compute and apply import-path updates before the rename.
    fn will_rename(&self, old: &Path, new: &Path) -> eyre::Result<()> {
        self.0.will_rename_file(old, new);
        Ok(())
    }

    /// Notify the LSP server that the rename has completed.
    fn did_rename(&self, old: &Path, new: &Path) { self.0.did_rename_file(old, new); }
}

/// Fragment node hook that attaches LSP `Renameable` to symbol directory nodes.
struct LspFragmentNodeHook;

/// Attaches LSP [`SymbolRename`] capability to symbol directory nodes at construction time.
impl FragmentNodeHook for LspFragmentNodeHook {
    /// If an LSP server is available, bind a [`SymbolQuery`] at the symbol's
    /// name offset and attach it as a [`Renameable`] on the node.
    fn augment(
        &self,
        node: VirtualNode,
        activation: &ActivationContext,
        source_file: &VfsPath,
        source: &str,
        name_byte_offset: usize,
    ) -> VirtualNode {
        let Some(handle) = Handle::for_file(activation, source_file) else {
            return node;
        };
        let query = handle.at(source, name_byte_offset);
        node.with_renameable(SymbolRename { query })
    }
}

/// Link-time registration of the LSP plugin into the global `PLUGINS` slice.
///
/// The binary's `main.rs` pulls in this crate with `use nyne_lsp as _;`,
/// which is enough for `linkme` to include this static in the final binary.
#[allow(unsafe_code)]
#[distributed_slice(PLUGINS)]
static LSP_PLUGIN: PluginFactory = || {
    Box::new(LspPlugin {
        resolved: OnceLock::new(),
    })
};
