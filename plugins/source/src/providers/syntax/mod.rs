use color_eyre::eyre;
use nyne::dispatch::invalidation::InvalidationEvent;
use nyne::dispatch::routing::ctx::RouteCtx;
use nyne::dispatch::routing::tree::RouteTree;
use nyne::provider::{MutationOp, MutationOutcome};
use nyne::templates::TemplateHandle;
use nyne::types::RealFs;
use nyne::types::path_conventions::split_companion_path;

use super::names::{self, SUBDIR_AT_LINE, SUBDIR_SYMBOLS, companion_name};
use super::prelude::*;
use crate::services::SourceServices;
use crate::syntax::SyntaxRegistry;
use crate::syntax::spec::Decomposer;
use crate::syntax::view::{SYMBOL_TABLE_PARTIAL_KEY, SYMBOL_TABLE_PARTIAL_SRC};

/// Optional delegate for file-rename coordination with external services.
///
/// Implementations are looked up from the [`ActivationContext`] `TypeMap`
/// at construction time. When present, the syntax provider calls the hook
/// around real file renames so that external systems (e.g. an LSP server)
/// can update import paths before and after the rename.
pub trait FileRenameHook: Send + Sync {
    /// Called **before** the actual file rename. The implementation may
    /// apply workspace edits (e.g. import-path updates) in response.
    fn will_rename(&self, old: &std::path::Path, new: &std::path::Path) -> eyre::Result<()>;

    /// Called **after** the file rename has completed on disk.
    fn did_rename(&self, old: &std::path::Path, new: &std::path::Path);
}

/// Content reading, writing, and rendering for decomposed symbols.
mod content;
/// Symbol lookup by shorthand, line number, and rename preview.
mod lookup;
/// Trailing newline middleware for symbol body reads and writes.
mod newline;
/// Symbol directory resolution — inventory, fragments, and LSP links.
mod resolve;

/// Core syntax decomposition provider — tree-sitter parsing, symbol resolution, and content access.
///
/// Owns the route tree that maps companion-namespace paths (`file.rs@/symbols/...`) to
/// virtual nodes, dispatching to the resolve, lookup, and content submodules. Each source
/// file gets its own companion directory tree with symbol inventory, meta-files (signature,
/// docstring, decorators), LSP feature nodes, and code-action diffs.
pub struct SyntaxProvider {
    ctx: Arc<ActivationContext>,
    overview: TemplateHandle,
    file_overview: TemplateHandle,
    rename_hook: Option<Arc<dyn FileRenameHook>>,
    routes: RouteTree<Self>,
}

/// Core construction and decomposition methods.
impl SyntaxProvider {
    /// Create a new syntax provider, registering all route trees and templates.
    pub(crate) fn new(ctx: Arc<ActivationContext>) -> Self {
        let mut b = names::handle_builder();
        b.register_partial(SYMBOL_TABLE_PARTIAL_KEY, SYMBOL_TABLE_PARTIAL_SRC);
        let overview_key = b.register("syntax/overview", include_str!("templates/overview.md.j2"));
        let file_overview_key = b.register("syntax/file_overview", include_str!("templates/file_overview.md.j2"));
        let engine = b.finish();
        Self {
            rename_hook: ctx.get::<Arc<dyn FileRenameHook>>().cloned(),
            ctx,
            overview: TemplateHandle::new(&engine, overview_key),
            file_overview: TemplateHandle::new(&engine, file_overview_key),
            routes: nyne_macros::routes!(Self, {
                // Root = companion root (file.rs@/)
                children(children_companion_root),
                lookup(lookup_companion_root),

                "symbols" => children_symbols_root {
                    lookup(lookup_symbols_root),

                    "by-kind" => children_by_kind_root {
                        "{kind}" => children_by_kind_filter,
                    }
                    "at-line" {
                        lookup(lookup_at_line),
                    }
                    "{..path}@" => children_fragment_dir {
                        lookup(lookup_fragment_dir),

                        "code" => children_code_block_dir,
                    }
                }
            }),
        }
    }

    /// Return a reference to the syntax registry.
    fn registry(&self) -> &SyntaxRegistry { &SourceServices::get(&self.ctx).syntax }

    /// Return the decomposer for a source file, if supported.
    fn decomposer_for(&self, source_file: &VfsPath) -> Option<&Arc<dyn Decomposer>> {
        self.registry().decomposer_for(source_file)
    }
}

use nyne::{companion_children, companion_lookup, source_file};

/// Route tree handler methods — thin wrappers that extract params and
/// delegate to the existing resolve/lookup methods.
impl SyntaxProvider {
    /// List children at the companion root level.
    fn children_companion_root(&self, ctx: &RouteCtx<'_>) -> Nodes {
        Ok(self.resolve_companion_root(&source_file(ctx)?, ctx))
    }

    /// Lookup a child node in the companion root.
    const fn lookup_companion_root(&self, _ctx: &RouteCtx<'_>, _name: &str) -> Node {
        // `lines` is in children (resolved); `lines:M-N` via LineSlice plugin derivation.
        Ok(None)
    }

    /// List all top-level symbols.
    fn children_symbols_root(&self, ctx: &RouteCtx<'_>) -> Nodes { self.resolve_symbols_root(&source_file(ctx)?, ctx) }

    /// Lookup a symbol by name in the symbols root.
    fn lookup_symbols_root(&self, ctx: &RouteCtx<'_>, name: &str) -> Node {
        let sf = source_file(ctx)?;
        if name == SUBDIR_AT_LINE {
            return Ok(Some(VirtualNode::directory(SUBDIR_AT_LINE)));
        }
        self.lookup_symbol_shorthand(&sf, name, ctx)
    }

    /// List distinct symbol kinds as directories.
    fn children_by_kind_root(&self, ctx: &RouteCtx<'_>) -> Nodes { self.resolve_by_kind_root(&source_file(ctx)?, ctx) }

    /// List symbols of a specific kind.
    fn children_by_kind_filter(&self, ctx: &RouteCtx<'_>) -> Nodes {
        self.resolve_by_kind_filter(&source_file(ctx)?, ctx.param("kind"), ctx)
    }

    /// Lookup a symbol by line number.
    fn lookup_at_line(&self, ctx: &RouteCtx<'_>, name: &str) -> Node {
        self.lookup_at_line_impl(&source_file(ctx)?, name, ctx)
    }

    /// List children of a fragment (symbol) directory.
    fn children_fragment_dir(&self, ctx: &RouteCtx<'_>) -> Nodes {
        self.resolve_fragment_dir(&source_file(ctx)?, ctx.params("path"), ctx)
    }

    /// Lookup a child node within a fragment directory.
    fn lookup_fragment_dir(&self, ctx: &RouteCtx<'_>, name: &str) -> Node {
        if name == "delete.diff" {
            return self.lookup_delete_preview(&source_file(ctx)?, ctx.params("path"), ctx);
        }
        Ok(None)
    }

    /// List fenced code block files for a document section.
    fn children_code_block_dir(&self, ctx: &RouteCtx<'_>) -> Nodes {
        self.resolve_code_block_dir(&source_file(ctx)?, ctx.params("path"), ctx)
    }
}

/// [`Provider`] implementation for [`SyntaxProvider`].
impl Provider for SyntaxProvider {
    /// Return the syntax provider identifier.
    fn id(&self) -> ProviderId { Self::PROVIDER_ID }

    /// Dispatch children through the companion route tree.
    fn children(self: Arc<Self>, ctx: &RequestContext<'_>) -> Nodes {
        let Some(split) = split_companion_path(ctx.path) else {
            return Ok(None);
        };
        companion_children(&self.routes, &self, ctx, &split)
    }

    /// Dispatch lookup through the companion route tree.
    fn lookup(self: Arc<Self>, ctx: &RequestContext<'_>, name: &str) -> Node {
        let Some(split) = split_companion_path(ctx.path) else {
            return Ok(None);
        };
        companion_lookup(&self.routes, &self, ctx, &split, name)
    }

    /// Intercept source file renames to coordinate with external services.
    fn handle_mutation(&self, op: &MutationOp<'_>, real_fs: &dyn RealFs) -> eyre::Result<MutationOutcome> {
        let MutationOp::Rename { from, to } = op else {
            return Ok(MutationOutcome::NotHandled);
        };

        // Only intercept renames of source files this provider recognizes.
        if self.decomposer_for(from).is_none() {
            return Ok(MutationOutcome::NotHandled);
        }

        let overlay_root = self.ctx.overlay_root();
        let old_path = overlay_root.join(from.as_str());
        let new_path = overlay_root.join(to.as_str());

        // Delegate pre-rename hook (e.g. LSP import-path updates).
        if let Some(hook) = &self.rename_hook {
            hook.will_rename(&old_path, &new_path)?;
        }

        // Perform the actual file rename.
        real_fs.rename(from, to)?;

        // Delegate post-rename notification.
        if let Some(hook) = &self.rename_hook {
            hook.did_rename(&old_path, &new_path);
        }

        Ok(MutationOutcome::Handled)
    }

    /// Invalidate decomposition caches for changed source files.
    fn on_fs_change(&self, changed: &[VfsPath]) -> Vec<InvalidationEvent> {
        let services = SourceServices::get(&self.ctx);
        changed
            .iter()
            .filter(|p| self.decomposer_for(p).is_some())
            .filter_map(|p| {
                services.decomposition.invalidate(p);
                let name = p.name()?;
                let parent = p.parent().unwrap_or(VfsPath::root());
                let companion_path = parent.join(&companion_name(name)).ok()?;
                let symbols_path = companion_path.join(SUBDIR_SYMBOLS).ok()?;
                Some([
                    InvalidationEvent::Subtree { path: companion_path },
                    InvalidationEvent::Subtree { path: symbols_path },
                ])
            })
            .flatten()
            .collect()
    }
}

/// Provider ID constant.
impl SyntaxProvider {
    /// Unique provider identifier for syntax decomposition.
    pub(crate) const PROVIDER_ID: ProviderId = ProviderId::new("syntax");
}
