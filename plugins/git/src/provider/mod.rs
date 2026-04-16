//! Git provider — exposes blame, history, log, contributors, diff, branches, and tags.
//!
//! Creates virtual files/directories under two scopes:
//! - `file.rs@/git/` — per-file git metadata (blame, log, contributors, notes)
//! - `@/git/` — repository-wide browsing (branches, tags, status)
//!
//! Both scopes are contributed via [`CompanionExtensions`] during plugin
//! activation. [`GitProvider`] handles post-dispatch companion rename
//! decoration and git directory change invalidation.
//!
//! Symbol-scoped git features (per-symbol blame/history) are registered
//! into [`SourceExtensions::fragment_path`] via [`symbol_routes`].
pub mod state;

use std::sync::Arc;

use color_eyre::eyre::Result;
use nyne::router::{Filesystem, Next, Provider, Request};
use nyne_companion::{CompanionProvider, CompanionRequest};
pub use branches::GitFileRename;
pub use state::*;

/// Branch browsing and mutation.
mod branches;
/// Diff generation.
mod diff;
/// Route tree and handler/content functions.
pub mod routes;
pub mod symbol_routes;

/// Working tree status rendering.
mod status;
/// History filename formatting and shared template constants.
pub mod views;

/// Git provider for post-dispatch companion rename decoration.
///
/// All git VFS content (per-file and mount-wide) is contributed via
/// [`CompanionExtensions`] during activation — this provider only
/// decorates file-level companion lookups with git-aware rename
/// capability.
pub struct GitProvider {
    pub(crate) state: Arc<GitState>,
    pub(crate) fs: Arc<dyn Filesystem>,
}

nyne::define_provider!(GitProvider, "git", deps: [CompanionProvider]);

/// [`nyne::router::Provider`] implementation for [`GitProvider`].
impl Provider for GitProvider {
    /// Per-file companion: run downstream, then decorate with rename capability.
    fn accept(&self, req: &mut Request, next: &Next) -> Result<()> {
        next.run(req)?;
        if req.companion().is_some_and(|c| c.source_file.is_some()) {
            routes::decorate_companion_rename(req, &self.state.repo, &self.fs);
        }
        Ok(())
    }
}

/// Unit tests.
#[cfg(test)]
mod tests;
