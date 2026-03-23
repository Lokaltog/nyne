use color_eyre::eyre;
use nyne::dispatch::invalidation::InvalidationEvent;
use nyne::dispatch::routing::ctx::RouteCtx;
use nyne::dispatch::routing::tree::RouteTree;
use nyne::provider::{MutationOp, MutationOutcome};
use nyne::templates::TemplateHandle;
use nyne::types::RealFs;
use nyne::types::path_conventions::split_companion_path;

use super::fragment_resolver::FragmentResolver;
use super::names::{self, COMPANION_SUFFIX, FILE_DIAGNOSTICS, FILE_HINTS, SUBDIR_AT_LINE, SUBDIR_SYMBOLS};
use super::prelude::*;
use crate::lsp::handle::LspHandle;
use crate::lsp::manager::LspManager;
use crate::syntax::SyntaxRegistry;
use crate::syntax::decomposed::DecompositionCache;
use crate::syntax::spec::Decomposer;
use crate::syntax::view::{SYMBOL_TABLE_PARTIAL_KEY, SYMBOL_TABLE_PARTIAL_SRC};

mod content;
mod lookup;
mod newline;
mod resolve;

use content::lsp::{LspFeature, LspHandles, build_diagnostics_node};

pub(crate) struct SyntaxProvider {
    ctx: Arc<ActivationContext>,
    overview: TemplateHandle,
    file_overview: TemplateHandle,
    hints: TemplateHandle,
    lsp: LspHandles,
    routes: RouteTree<Self>,
}

impl SyntaxProvider {
    pub(crate) fn new(ctx: Arc<ActivationContext>) -> Self {
        let mut b = names::handle_builder();
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
        let lsp_keys = [
            b.register("syntax/lsp/definition", include_str!("templates/lsp/definition.md.j2")),
            b.register(
                "syntax/lsp/declaration",
                include_str!("templates/lsp/declaration.md.j2"),
            ),
            b.register(
                "syntax/lsp/type_definition",
                include_str!("templates/lsp/type_definition.md.j2"),
            ),
            b.register("syntax/lsp/references", include_str!("templates/lsp/references.md.j2")),
            b.register(
                "syntax/lsp/implementation",
                include_str!("templates/lsp/implementation.md.j2"),
            ),
            b.register("syntax/lsp/callers", include_str!("templates/lsp/callers.md.j2")),
            b.register("syntax/lsp/deps", include_str!("templates/lsp/deps.md.j2")),
            b.register("syntax/lsp/supertypes", include_str!("templates/lsp/supertypes.md.j2")),
            b.register("syntax/lsp/subtypes", include_str!("templates/lsp/subtypes.md.j2")),
            b.register("syntax/lsp/doc", include_str!("templates/lsp/doc.md.j2")),
            b.register("syntax/lsp/hints", include_str!("templates/lsp/hints.md.j2")),
        ];
        let diagnostics_key = b.register(
            "syntax/lsp/diagnostics",
            include_str!("templates/lsp/diagnostics.md.j2"),
        );
        let hints_key = b.register("syntax/hints", include_str!("templates/hints.md.j2"));
        let engine = b.finish();
        let overview = TemplateHandle::new(&engine, overview_key);
        let file_overview = TemplateHandle::new(&engine, file_overview_key);
        let hints = TemplateHandle::new(&engine, hints_key);
        let lsp = LspHandles {
            features: lsp_keys.map(|key| TemplateHandle::new(&engine, key)),
            diagnostics: TemplateHandle::new(&engine, diagnostics_key),
        };

        let routes = nyne_macros::routes!(Self, {
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
        });

        Self {
            ctx,
            overview,
            file_overview,
            hints,
            lsp,
            routes,
        }
    }

    #[expect(
        clippy::expect_used,
        reason = "returns &SyntaxRegistry, not Result — programming error if missing"
    )]
    fn registry(&self) -> &SyntaxRegistry {
        self.ctx
            .get::<Arc<SyntaxRegistry>>()
            .expect("coding plugin not activated")
    }

    fn decomposer_for(&self, source_file: &VfsPath) -> Option<&Arc<dyn Decomposer>> {
        self.registry().decomposer_for(source_file)
    }

    /// Build the file-level HINTS.md node (lookup-only).
    #[expect(
        clippy::expect_used,
        reason = "returns Option, not Result — programming error if missing"
    )]
    fn build_hints_node(&self, source_file: &VfsPath) -> Option<VirtualNode> {
        let cache = self
            .ctx
            .get::<DecompositionCache>()
            .expect("coding plugin not activated")
            .clone();
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
    fn children_companion_root(&self, ctx: &RouteCtx<'_>) -> Nodes {
        Ok(self.resolve_companion_root(&source_file(ctx)?, ctx))
    }

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

    fn children_symbols_root(&self, ctx: &RouteCtx<'_>) -> Nodes { self.resolve_symbols_root(&source_file(ctx)?, ctx) }

    fn lookup_symbols_root(&self, ctx: &RouteCtx<'_>, name: &str) -> Node {
        let sf = source_file(ctx)?;
        if name == SUBDIR_AT_LINE {
            return Ok(Some(VirtualNode::directory(SUBDIR_AT_LINE)));
        }
        self.lookup_symbol_shorthand(&sf, name, ctx)
    }

    fn children_by_kind_root(&self, ctx: &RouteCtx<'_>) -> Nodes { self.resolve_by_kind_root(&source_file(ctx)?, ctx) }

    fn children_by_kind_filter(&self, ctx: &RouteCtx<'_>) -> Nodes {
        let kind = ctx.param("kind");
        self.resolve_by_kind_filter(&source_file(ctx)?, kind, ctx)
    }

    fn lookup_at_line(&self, ctx: &RouteCtx<'_>, name: &str) -> Node {
        self.lookup_at_line_impl(&source_file(ctx)?, name, ctx)
    }

    fn children_fragment_dir(&self, ctx: &RouteCtx<'_>) -> Nodes {
        let path = ctx.params("path");
        self.resolve_fragment_dir(&source_file(ctx)?, path, ctx)
    }

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

    fn lookup_rename_preview(&self, ctx: &RouteCtx<'_>, name: &str) -> Node {
        let path = ctx.params("path");
        self.lookup_rename_preview_impl(&source_file(ctx)?, path, name, ctx)
    }

    fn lookup_file_rename_preview(&self, ctx: &RouteCtx<'_>, name: &str) -> Node {
        self.lookup_file_rename_preview_impl(&source_file(ctx)?, name)
    }

    fn children_actions_dir(&self, ctx: &RouteCtx<'_>) -> Nodes {
        let path = ctx.params("path");
        self.resolve_actions_dir(&source_file(ctx)?, path, ctx)
    }

    fn children_code_block_dir(&self, ctx: &RouteCtx<'_>) -> Nodes {
        let path = ctx.params("path");
        self.resolve_code_block_dir(&source_file(ctx)?, path, ctx)
    }

    fn children_lsp_dir(&self, ctx: &RouteCtx<'_>) -> Nodes {
        let lsp_dir = ctx.param("lsp_dir");
        if LspFeature::from_dir_name(lsp_dir).is_none() {
            return Ok(None);
        }
        let path = ctx.params("path");
        self.resolve_lsp_symlink_dir(&source_file(ctx)?, path, lsp_dir, ctx)
    }
}

impl Provider for SyntaxProvider {
    fn id(&self) -> ProviderId { Self::PROVIDER_ID }

    fn children(self: Arc<Self>, ctx: &RequestContext<'_>) -> Nodes {
        let Some(split) = split_companion_path(ctx.path) else {
            return Ok(None);
        };
        companion_children(&self.routes, &self, ctx, &split)
    }

    fn lookup(self: Arc<Self>, ctx: &RequestContext<'_>, name: &str) -> Node {
        let Some(split) = split_companion_path(ctx.path) else {
            return Ok(None);
        };
        companion_lookup(&self.routes, &self, ctx, &split, name)
    }

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

        #[expect(clippy::expect_used, reason = "programming error if missing")]
        let lsp = self.ctx.get::<Arc<LspManager>>().expect("coding plugin not activated");

        // Ask the LSP server for import-path updates BEFORE the rename.
        lsp.will_rename_file(&old_path, &new_path);

        // Perform the actual file rename.
        real_fs.rename(from, to)?;

        // Notify the LSP server AFTER the rename completed.
        lsp.did_rename_file(&old_path, &new_path);

        Ok(MutationOutcome::Handled)
    }

    #[expect(
        clippy::expect_used,
        reason = "returns Vec, not Result — programming error if missing"
    )]
    fn on_fs_change(&self, changed: &[VfsPath]) -> Vec<InvalidationEvent> {
        changed
            .iter()
            .filter(|p| self.decomposer_for(p).is_some())
            .filter_map(|p| {
                // Evict the cached decomposition for the changed file.
                self.ctx
                    .get::<DecompositionCache>()
                    .expect("coding plugin not activated")
                    .invalidate(p);

                // Notify the LSP server: sends didChange for open documents
                // (keeping them open with incremented version) and invalidates
                // the LSP result cache. On file deletion, falls back to didClose.
                let lsp_file = self.ctx.overlay_root().join(p.as_str());
                self.ctx
                    .get::<Arc<LspManager>>()
                    .expect("coding plugin not activated")
                    .invalidate_file(&lsp_file);
                let name = p.name()?;
                let parent = p.parent().unwrap_or(VfsPath::root());
                let companion = format!("{name}{COMPANION_SUFFIX}");
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

impl SyntaxProvider {
    pub(crate) const PROVIDER_ID: ProviderId = ProviderId::new("syntax");
}
