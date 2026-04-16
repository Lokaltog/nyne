//! Plugin registration and activation for the git plugin.
//!
//! Opens the git repository during activation, builds shared state, and
//! registers per-file companion content via [`CompanionExtensions`].

pub mod config;

use std::sync::Arc;

use linkme::distributed_slice;
use nyne::ExtensionCounts;
use nyne::config::PluginConfig;
use nyne::plugin::PluginFactory;
use nyne::prelude::*;
use nyne::router::Provider;
use nyne_companion::CompanionContextExt;
use nyne_source::SourceContextExt;
use tracing::{debug, warn};

use self::config::Config;
use crate::context::GitContextExt;
use crate::provider::{self, GitProvider, GitState};
use crate::repo::Repo;

/// Git plugin entry point — opens the repo and creates providers.
///
/// During activation, discovers the git repository for the source directory
/// and inserts a shared [`Repo`] into the `AnyMap`. If no repo is found,
/// gracefully disables itself by returning no providers.
pub struct GitPlugin;

/// [`Plugin`] implementation for [`GitPlugin`].
impl Plugin for GitPlugin {
    nyne::provider_graph!(GitProvider);

    nyne::plugin_config!(Config);

    /// Returns the plugin identifier.
    fn id(&self) -> &'static str { "git" }

    /// Opens the git repo, builds shared state, and registers companion extensions.
    fn activate(&self, ctx: &mut ActivationContext) -> Result<()> {
        let repo = match Repo::open(ctx.source_root()) {
            Ok(repo) => Arc::new(repo),
            Err(e) => {
                debug!("no git repo: {e}");
                return Ok(());
            }
        };

        debug!("git repo opened at {}", ctx.source_root().display());

        match repo.extension_counts() {
            Ok(counts) => ctx.insert(ExtensionCounts::new(counts)),
            Err(e) => warn!(error = %e, "failed to read extension counts from git index"),
        }
        let config = Config::from_context(ctx, self.id());
        let state = Arc::new(GitState {
            repo: Arc::clone(&repo),
            handles: provider::build_handles(&config.vfs),
            limits: config.limits,
            vfs: config.vfs,
        });

        // Register companion content — per-file (git/, diff/, history/) and
        // mount-wide (git/ branches, tags, status).
        provider::routes::register_companion_extensions(ctx.companion_extensions_mut(), &state);
        provider::routes::register_mount_extensions(ctx.companion_extensions_mut(), &state);

        // Register symbol-scoped git content (per-symbol blame, log, history)
        // into the source plugin's fragment_path extension point. Gracefully
        // skipped when the source plugin is not loaded.
        if let (Some(decomposition), Some(syntax)) =
            (ctx.decomposition_cache().cloned(), ctx.syntax_registry().cloned())
        {
            provider::symbol_routes::register_source_extensions(
                ctx.source_extensions_mut(),
                &state,
                &decomposition,
                &syntax,
            );
        }

        ctx.insert(state);
        ctx.insert(repo);

        Ok(())
    }

    fn providers(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<Arc<dyn Provider>>> {
        let Some(state) = ctx.git_state().cloned() else {
            return Ok(vec![]);
        };
        Ok(vec![Arc::new(GitProvider {
            state,
            fs: Arc::clone(ctx.fs()),
        })])
    }
}

/// Plugin factory registered via `linkme` distributed slice.
#[allow(unsafe_code)]
#[distributed_slice(PLUGINS)]
static GIT_PLUGIN: PluginFactory = || Box::new(GitPlugin);
