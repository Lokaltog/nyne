//! LSP VFS providers -- bridge between LSP intelligence and the virtual filesystem.
//!
//! Two providers live here:
//! - [`LspRenameProvider`] wraps source file renames with LSP coordination
//!   (`willRenameFiles` / `didRenameFiles`). Runs before `SyntaxProvider` in
//!   the middleware chain so it can bracket the actual filesystem rename.
//! - [`LspProvider`] contributes LSP-powered nodes (CALLERS.md, DEPS.md,
//!   REFERENCES.md, rename/, actions/, DIAGNOSTICS.md) to symbol directories owned
//!   by `SyntaxProvider` via multi-provider composition.
//!
//! Workspace symbol search (`@/search/symbols/{query}`) is registered as a
//! mount-wide companion extension in [`routes::register_mount_extensions`].

pub mod content;
mod lsp_links;
pub mod routes;
pub mod state;

use std::path::PathBuf;
use std::sync::Arc;

use color_eyre::eyre;
use nyne::router::{InvalidationEvent, Next, Op, Provider, Request};
use nyne_companion::{CompanionProvider, CompanionRequest};
use nyne_source::SyntaxProvider;
pub use state::LspState;

/// Middleware provider that wraps source file renames with LSP coordination.
///
/// Declares a dependency on [`CompanionProvider`] (not [`SyntaxProvider`]) so
/// it runs **before** `SyntaxProvider` in the middleware chain. This lets it
/// call `will_rename_file` before the filesystem rename and `did_rename_file`
/// after.
pub struct LspRenameProvider {
    pub(crate) state: Arc<LspState>,
}

nyne::define_provider!(LspRenameProvider, "lsp-rename", deps: [CompanionProvider]);

impl Provider for LspRenameProvider {
    fn accept(&self, req: &mut Request, next: &Next) -> eyre::Result<()> {
        if let Op::Rename {
            target_dir,
            target_name,
            ..
        } = req.op()
            && let Some(companion) = req.companion()
            && let Some(source_file) = &companion.source_file
            && self.state.syntax.decomposer_for(source_file).is_some()
        {
            let source_root = self.state.lsp.path_resolver().source_root();
            let old = source_root.join(source_file);
            let new = source_root.join(target_dir.join(target_name));
            self.state.lsp.will_rename_file(&old, &new);
            next.run(req)?;
            self.state.lsp.did_rename_file(&old, &new);
            return Ok(());
        }
        next.run(req)
    }
}

/// LSP provider — minimal provider for `on_change` invalidation.
///
/// All LSP VFS nodes (CALLERS.md, DEPS.md, DIAGNOSTICS.md, actions/, rename/,
/// etc.) are contributed via [`CompanionExtensions`] and [`SourceExtensions`]
/// registered during `activate()`. This provider exists solely to forward
/// file-change notifications to the LSP manager.
pub struct LspProvider {
    pub(crate) state: Arc<LspState>,
}

nyne::define_provider!(LspProvider, "lsp", deps: [SyntaxProvider]);

/// [`nyne::router::Provider`] implementation for [`LspProvider`].
///
/// All LSP VFS nodes are contributed via extension points registered during
/// `activate()`. This provider exists solely to forward file-change
/// notifications to the LSP manager for cache invalidation.
impl Provider for LspProvider {
    fn on_change(&self, changed: &[PathBuf]) -> Vec<InvalidationEvent> {
        for p in changed {
            if self.state.syntax.decomposer_for(p).is_some() {
                self.state
                    .lsp
                    .invalidate_file(&self.state.lsp.path_resolver().source_root().join(p));
            }
        }
        // Companion path invalidation is handled by CompanionProvider.
        Vec::new()
    }
}
