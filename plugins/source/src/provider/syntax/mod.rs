//! Core syntax decomposition provider — tree-sitter parsing, symbol resolution,
//! and content access.
//!
//! Owns the route tree that maps companion-namespace paths (`file.rs@/symbols/...`)
//! to virtual nodes, dispatching to the resolve, lookup, and content submodules.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use color_eyre::eyre;
#[allow(clippy::redundant_pub_crate)]
pub(crate) use content::file_doc_text;
use nyne::router::tree::RouteTree;
use nyne::router::{Filesystem, InvalidationEvent, Next, Op, Provider, Request};
use nyne::templates::TemplateHandle;
use nyne_companion::{CompanionProvider, CompanionRequest};

use crate::edit::staging::EditStaging;
use crate::plugin::config::vfs::Vfs;
use crate::syntax::SyntaxRegistry;
use crate::syntax::decomposed::DecompositionCache;
use crate::syntax::spec::Decomposer;

/// Content reading, writing, and rendering for decomposed symbols.
mod content;
/// Symbol lookup by shorthand, line number, and rename preview.
mod lookup;

/// Symbol directory resolution — inventory, fragments, and LSP links.
mod resolve;
/// Route tree and handler/content functions.
#[allow(clippy::redundant_pub_crate)]
pub(crate) mod routes;

/// Core syntax decomposition provider.
///
/// Each source file gets its own companion directory tree with symbol inventory,
/// meta-files (signature, docstring, decorators), and code-action diffs.
pub struct SyntaxProvider {
    pub(crate) registry: Arc<SyntaxRegistry>,
    pub(crate) decomposition: DecompositionCache,
    pub(crate) staging: EditStaging,
    pub(crate) fs: Arc<dyn Filesystem>,
    pub(crate) vfs: Vfs,
    pub(crate) overview: TemplateHandle,
    pub(crate) file_overview: TemplateHandle,
    pub(crate) tree: RouteTree<Self>,
}

nyne::define_provider!(SyntaxProvider, "syntax", deps: [CompanionProvider]);

impl SyntaxProvider {
    /// Return the decomposer for a source file, if supported.
    fn decomposer_for(&self, source_file: &Path) -> Option<&Arc<dyn Decomposer>> {
        self.registry.decomposer_for(source_file)
    }

    /// Handle a source file rename.
    ///
    /// Returns `Some(Ok(()))` if the rename was handled, `Some(Err(...))` on
    /// failure, or `None` if this file isn't ours to rename.
    fn handle_rename(&self, source_file: &Path, target_dir: &Path, target_name: &str) -> Option<eyre::Result<()>> {
        self.decomposer_for(source_file)?;
        Some(self.fs.rename(source_file, &target_dir.join(target_name)))
    }
}

impl Provider for SyntaxProvider {
    fn accept(&self, req: &mut Request, next: &Next) -> eyre::Result<()> {
        let Some(companion) = req.companion() else {
            return next.run(req);
        };
        let Some(ref source_file) = companion.source_file else {
            return next.run(req);
        };

        // Intercept source file renames — the actual filesystem rename.
        // handle_rename guards on decomposer_for() internally.
        if let Op::Rename {
            target_dir,
            target_name,
            ..
        } = req.op()
            && let Some(result) = self.handle_rename(source_file, target_dir, target_name)
        {
            return result;
        }

        // Only activate for files with a registered syntax decomposer.
        // Directories and non-parseable files (images, binaries) skip
        // the entire route tree so auto_emit doesn't leak virtual nodes.
        if self.decomposer_for(source_file).is_none() {
            return next.run(req);
        }

        self.tree.dispatch(self, req, next)
    }

    fn on_change(&self, changed: &[PathBuf]) -> Vec<InvalidationEvent> {
        for p in changed {
            if self.decomposer_for(p).is_some() {
                self.decomposition.invalidate(p);
            }
        }
        // Companion path invalidation is handled by CompanionProvider.
        Vec::new()
    }
}
