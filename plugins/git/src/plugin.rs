use std::path::Path;
use std::sync::Arc;

use color_eyre::eyre::Result;
use linkme::distributed_slice;
use nyne::dispatch::activation::ActivationContext;
use nyne::plugin::{PLUGINS, Plugin, PluginFactory};
use nyne::provider::Provider;
use nyne::types::{ExtensionCounts, GitDirName};
use tracing::{debug, warn};

use crate::companion::GitCompanionProvider;
use crate::provider::GitProvider;
use crate::repo::GitRepo;

/// Git plugin entry point — opens the repo and creates providers.
pub struct GitPlugin;

/// [`Plugin`] implementation for [`GitPlugin`].
impl Plugin for GitPlugin {
    /// Returns the plugin identifier.
    fn id(&self) -> &'static str { "git" }

    /// Opens the git repo and inserts shared state into the activation context.
    fn activate(&self, ctx: &mut ActivationContext) -> Result<()> {
        match GitRepo::open(ctx.overlay_root()) {
            Ok(repo) => {
                let repo = Arc::new(repo);
                debug!("git repo opened at {}", ctx.overlay_root().display());

                ctx.insert(ExtensionCounts(repo.extension_counts()));
                ctx.insert(GitDirName(git_dir_component(ctx.overlay_root(), &repo.git_dir_path())));
                ctx.insert(repo);
            }
            Err(e) => {
                debug!("no git repo: {e}");
            }
        }
        Ok(())
    }

    /// Creates git and git-companion providers if a repo is available.
    fn providers(&self, ctx: &Arc<ActivationContext>) -> Result<Vec<Arc<dyn Provider>>> {
        if ctx.get::<Arc<GitRepo>>().is_none() {
            return Ok(vec![]);
        }
        Ok(vec![
            Arc::new(GitProvider::new(Arc::clone(ctx))),
            Arc::new(GitCompanionProvider::new(Arc::clone(ctx))),
        ])
    }
}

/// Derive the VFS-relative git directory name from the repo's git dir path.
///
/// For a normal repo at `/project/.git/`, returns `".git"`.
/// Returns `None` if the git dir is outside the project tree or is the root itself.
fn git_dir_component(overlay_root: &Path, git_dir_path: &Path) -> Option<String> {
    // `repo.path()` may have a trailing slash — normalize.
    let git_path = git_dir_path
        .to_str()
        .map_or(git_dir_path, |s| Path::new(s.trim_end_matches('/')));

    let relative = git_path.strip_prefix(overlay_root).ok()?;
    let first = relative.components().next()?;
    let name = first.as_os_str().to_str()?;

    if relative.components().count() > 1 {
        warn!(
            git_path = %git_path.display(),
            "git directory is nested — using first component for filter"
        );
    }

    Some(name.to_owned())
}

/// Plugin factory registered via `linkme` distributed slice.
#[allow(unsafe_code)]
#[distributed_slice(PLUGINS)]
static GIT_PLUGIN: PluginFactory = || Box::new(GitPlugin);
