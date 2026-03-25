use color_eyre::eyre;
use nyne::dispatch::invalidation::InvalidationEvent;
use nyne::dispatch::routing::ctx::RouteCtx;
use nyne::dispatch::routing::tree::RouteTree;
use nyne::provider::{MutationOp, MutationOutcome};
use nyne::templates::TemplateHandle;
use nyne::types::RealFs;
use nyne::types::path_conventions::split_companion_path;

use super::fragment_resolver::FragmentResolver;
use super::names::{self, FILE_DIAGNOSTICS, FILE_HINTS, SUBDIR_AT_LINE, SUBDIR_SYMBOLS, companion_name};
use super::prelude::*;
use crate::lsp::handle::LspHandle;
use crate::services::CodingServices;
use crate::syntax::SyntaxRegistry;
use crate::syntax::spec::Decomposer;
use crate::syntax::view::{SYMBOL_TABLE_PARTIAL_KEY, SYMBOL_TABLE_PARTIAL_SRC};

/// Content reading, writing, and rendering for decomposed symbols.
mod content;
/// Symbol lookup by shorthand, line number, and rename preview.
mod lookup;
/// Trailing newline middleware for symbol body reads and writes.
mod newline;
/// Symbol directory resolution — inventory, fragments, and LSP links.
mod resolve;

use std::array;

use content::lsp::{LspFeature, LspHandles, build_diagnostics_node};
use strum::IntoEnumIterator;

/// Core syntax decomposition provider — tree-sitter parsing, symbol resolution, and LSP integration.
pub(crate) struct SyntaxProvider {
    ctx: Arc<ActivationContext>,
    overview: TemplateHandle,
    file_overview: TemplateHandle,
    hints: TemplateHandle,
    lsp: LspHandles,
    routes: RouteTree<Self>,
}

/// Core construction and decomposition methods.
impl SyntaxProvider {
    /// Create a new syntax provider, registering all route trees and templates.
    pub(crate) fn new(ctx: Arc<ActivationContext>) -> Self {
        let mut b = names::handle_builder();
        LspFeature::register_globals(b.engine_mut());
        // Shared partials (included by individual LSP templates).
        b.register_partial(SYMBOL_TABLE_PARTIAL_KEY, SYMBOL_TABLE_PARTIAL_SRC);
        b.register_partial("syntax/lsp/_locations", include_str!("templates/lsp/_locations.md.j2"));
        b.register_partial(
            "syntax/lsp/_type_hierarchy",
            include_str!("templates/lsp/_type_hierarchy.md.j2"),
        );
        // Per-feature templates.
        let overview_key = b.register("syntax/overview", include_str!("templates/overview.md.j2"));
        let file_overview_key = b.register("syntax/file_overview", include_str!("templates/file_overview.md.j2"));
        // Per-feature LSP templates — order derived from LspFeature::iter().
        let lsp_keys: Vec<_> = LspFeature::iter()
            .map(|f| {
                let (name, src) = f.template();
                b.register(name, src)
            })
            .collect();
        let diagnostics_key = b.register(
            "syntax/lsp/diagnostics",
            include_str!("templates/lsp/diagnostics.md.j2"),
        );
        let hints_key = b.register("syntax/hints", include_str!("templates/hints.md.j2"));
        let engine = b.finish();
        let mut lsp_keys = lsp_keys.into_iter();
        Self {
            ctx,
            overview: TemplateHandle::new(&engine, overview_key),
            file_overview: TemplateHandle::new(&engine, file_overview_key),
            hints: TemplateHandle::new(&engine, hints_key),
            lsp: LspHandles {
                #[expect(clippy::expect_used, reason = "length matches LspFeature::COUNT by construction")]
                features: array::from_fn(|_| {
                    TemplateHandle::new(&engine, lsp_keys.next().expect("LspFeature::COUNT mismatch"))
                }),
                diagnostics: TemplateHandle::new(&engine, diagnostics_key),
            },
            routes: nyne_macros::routes!(Self, {
                // Root = companion root (file.rs@/)
                children(children_companion_root),
                lookup(lookup_companion_root),

                "rename" {
                    lookup(lookup_file_rename_preview),
                }

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

                        "rename" {
                            lookup(lookup_rename_preview),
                        }
                        "actions" => children_actions_dir,
                        "code" => children_code_block_dir,
                        "{lsp_dir}" => children_lsp_dir,
                    }
                }
            }),
        }
    }

    /// Return a reference to the syntax registry.
    fn registry(&self) -> &SyntaxRegistry { &CodingServices::get(&self.ctx).syntax }

    /// Return the decomposer for a source file, if supported.
    fn decomposer_for(&self, source_file: &VfsPath) -> Option<&Arc<dyn Decomposer>> {
        self.registry().decomposer_for(source_file)
    }

    /// Build the file-level HINTS.md node (lookup-only).
    fn build_hints_node(&self, source_file: &VfsPath) -> Option<VirtualNode> {
        let cache = CodingServices::get(&self.ctx).decomposition.clone();
        // Only produce hints for files with a parse tree.
        cache.get(source_file).ok()?.tree.as_ref()?;
        let resolver = FragmentResolver::new(cache, source_file.clone());
        Some(self.hints.node(FILE_HINTS, content::hints::HintsContent {
            resolver,
            activation: Arc::clone(&self.ctx),
        }))
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
    fn lookup_companion_root(&self, ctx: &RouteCtx<'_>, name: &str) -> Node {
        let sf = source_file(ctx)?;
        // DIAGNOSTICS.md — lookup-only, hidden from readdir.
        if name == FILE_DIAGNOSTICS {
            return Ok(
                LspHandle::for_file(&self.ctx, &sf).map(|h| build_diagnostics_node(FILE_DIAGNOSTICS, &h, &self.lsp))
            );
        }
        // HINTS.md — lookup-only, runs analysis at read time.
        if name == FILE_HINTS {
            return Ok(self.build_hints_node(&sf));
        }
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
        let kind = ctx.param("kind");
        self.resolve_by_kind_filter(&source_file(ctx)?, kind, ctx)
    }

    /// Lookup a symbol by line number.
    fn lookup_at_line(&self, ctx: &RouteCtx<'_>, name: &str) -> Node {
        self.lookup_at_line_impl(&source_file(ctx)?, name, ctx)
    }

    /// List children of a fragment (symbol) directory.
    fn children_fragment_dir(&self, ctx: &RouteCtx<'_>) -> Nodes {
        let path = ctx.params("path");
        self.resolve_fragment_dir(&source_file(ctx)?, path, ctx)
    }

    /// Lookup a child node within a fragment directory.
    fn lookup_fragment_dir(&self, ctx: &RouteCtx<'_>, name: &str) -> Node {
        let sf = source_file(ctx)?;
        let path = ctx.params("path");
        if name == "delete.diff" {
            return self.lookup_delete_preview(&sf, path, ctx);
        }
        // rename/ is lookup-only (not in readdir) — emit bare directory
        // when LSP is available.
        if name == "rename" {
            return Ok(LspHandle::for_file(&self.ctx, &sf)
                .is_some()
                .then(|| VirtualNode::directory(name)));
        }
        Ok(None)
    }

    /// Lookup a symbol rename preview diff by name.
    fn lookup_rename_preview(&self, ctx: &RouteCtx<'_>, name: &str) -> Node {
        let path = ctx.params("path");
        self.lookup_rename_preview_impl(&source_file(ctx)?, path, name, ctx)
    }

    /// Lookup a file rename preview diff by name.
    fn lookup_file_rename_preview(&self, ctx: &RouteCtx<'_>, name: &str) -> Node {
        self.lookup_file_rename_preview_impl(&source_file(ctx)?, name)
    }

    /// List LSP code action nodes for a symbol.
    fn children_actions_dir(&self, ctx: &RouteCtx<'_>) -> Nodes {
        let path = ctx.params("path");
        self.resolve_actions_dir(&source_file(ctx)?, path, ctx)
    }

    /// List fenced code block files for a document section.
    fn children_code_block_dir(&self, ctx: &RouteCtx<'_>) -> Nodes {
        let path = ctx.params("path");
        self.resolve_code_block_dir(&source_file(ctx)?, path, ctx)
    }

    /// List LSP feature nodes for a symbol.
    fn children_lsp_dir(&self, ctx: &RouteCtx<'_>) -> Nodes {
        let lsp_dir = ctx.param("lsp_dir");
        if LspFeature::from_dir_name(lsp_dir).is_none() {
            return Ok(None);
        }
        let path = ctx.params("path");
        self.resolve_lsp_symlink_dir(&source_file(ctx)?, path, lsp_dir, ctx)
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

    /// Intercept source file renames to coordinate with the LSP server.
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

        let lsp = &CodingServices::get(&self.ctx).lsp;

        // Ask the LSP server for import-path updates BEFORE the rename.
        lsp.will_rename_file(&old_path, &new_path);

        // Perform the actual file rename.
        real_fs.rename(from, to)?;

        // Notify the LSP server AFTER the rename completed.
        lsp.did_rename_file(&old_path, &new_path);

        Ok(MutationOutcome::Handled)
    }

    /// Invalidate decomposition and LSP caches for changed source files.
    fn on_fs_change(&self, changed: &[VfsPath]) -> Vec<InvalidationEvent> {
        let services = CodingServices::get(&self.ctx);
        changed
            .iter()
            .filter(|p| self.decomposer_for(p).is_some())
            .filter_map(|p| {
                // Evict the cached decomposition for the changed file.
                services.decomposition.invalidate(p);

                // Notify the LSP server: sends didChange for open documents
                // (keeping them open with incremented version) and invalidates
                // the LSP result cache. On file deletion, falls back to didClose.
                let lsp_file = self.ctx.overlay_root().join(p.as_str());
                services.lsp.invalidate_file(&lsp_file);
                let name = p.name()?;
                let parent = p.parent().unwrap_or(VfsPath::root());
                let companion = companion_name(name);
                let companion_path = parent.join(&companion).ok()?;
                let symbols_path = companion_path.join(SUBDIR_SYMBOLS).ok()?;
                // Invalidate both the companion root (file-level OVERVIEW.md)
                // and the symbols subtree (per-symbol nodes).
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
