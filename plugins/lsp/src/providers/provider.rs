//! LSP provider — contributes LSP-powered nodes to symbol directories.
//!
//! Progressive disclosure: when this provider is loaded, symbol directories
//! gain LSP-backed content nodes (CALLERS.md, DEPS.md, REFERENCES.md, etc.),
//! rename previews, code action diffs, and file-level DIAGNOSTICS.md. Without
//! it, only base syntax nodes appear.

use std::array;
use std::sync::Arc;

use nyne::dispatch::activation::ActivationContext;
use nyne::dispatch::context::RequestContext;
use nyne::dispatch::invalidation::InvalidationEvent;
use nyne::dispatch::routing::ctx::RouteCtx;
use nyne::dispatch::routing::tree::RouteTree;
use nyne::node::VirtualNode;
use nyne::provider::{Node, Nodes, Provider, ProviderId};
use nyne::templates::TemplateHandle;
use nyne::types::path_conventions::split_companion_path;
use nyne::types::vfs_path::VfsPath;
use nyne::{companion_children, companion_lookup, source_file};
use nyne_source::edit::diff_action::DiffActionNode;
use nyne_source::providers::fragment_resolver::FragmentResolver;
use nyne_source::providers::names::{SUBDIR_SYMBOLS, companion_name, handle_builder};
use nyne_source::services::SourceServices;
use nyne_source::syntax::{SyntaxRegistry, find_fragment};
use strum::IntoEnumIterator;

const FILE_DIAGNOSTICS: &str = "DIAGNOSTICS.md";
const SUBDIR_ACTIONS: &str = "actions";

use crate::lsp::handle::LspHandle;
use crate::lsp::manager::LspManager;
use crate::providers::content::rename::{FileRenameDiff, RenameDiff};
use crate::providers::content::{LspFeature, LspHandles, build_diagnostics_node, build_lsp_symbol_nodes};
use crate::providers::lsp_links;

/// LSP provider — contributes LSP-powered nodes to companion symbol directories.
///
/// Works alongside `SyntaxProvider` via multi-provider composition. The dispatch
/// layer auto-merges directory children from both providers, so symbol directories
/// gain LSP nodes when this plugin is loaded.
pub struct LspProvider {
    ctx: Arc<ActivationContext>,
    lsp: LspHandles,
    routes: RouteTree<Self>,
}

impl LspProvider {
    pub(crate) const PROVIDER_ID: ProviderId = ProviderId::new("lsp");

    /// Create a new LSP provider, registering all LSP templates.
    pub(crate) fn new(ctx: Arc<ActivationContext>) -> Self {
        let mut b = handle_builder();
        nyne::register_globals!(b.engine_mut(), FILE_DIAGNOSTICS, SUBDIR_ACTIONS,);

        // Shared partials (included by individual LSP templates).
        b.register_partial("syntax/lsp/_locations", include_str!("templates/lsp/_locations.md.j2"));

        // Register file name globals so templates can reference e.g. FILE_DEFINITION.
        LspFeature::register_globals(b.engine_mut());

        // Per-feature LSP templates — order derived from LspFeature::iter().
        let lsp_keys: Vec<_> = LspFeature::iter()
            .map(|f| {
                let (name, src) = f.template();
                b.register(name, src)
            })
            .collect();

        // File-level diagnostics template.
        let diagnostics_key = b.register(
            "syntax/lsp/diagnostics",
            include_str!("templates/lsp/diagnostics.md.j2"),
        );

        let engine = b.finish();
        let mut lsp_keys = lsp_keys.into_iter();

        Self {
            ctx,
            lsp: LspHandles {
                #[expect(clippy::expect_used, reason = "length matches LspFeature::COUNT by construction")]
                features: array::from_fn(|_| {
                    TemplateHandle::new(&engine, lsp_keys.next().expect("LspFeature::COUNT mismatch"))
                }),
                diagnostics: TemplateHandle::new(&engine, diagnostics_key),
            },
            routes: nyne_macros::routes!(Self, {
                // Root = companion root (file.rs@/)
                lookup(lookup_companion_root),

                "rename" {
                    lookup(lookup_file_rename_preview),
                }

                "symbols" {
                    "{..path}@" => children_fragment_lsp {
                        lookup(lookup_fragment_lsp),

                        "rename" {
                            lookup(lookup_rename_preview),
                        }
                        "actions" => children_actions_dir,
                        "{lsp_dir}" => children_lsp_dir,
                    }
                }
            }),
        }
    }

    /// Return the source services from the activation context.
    fn services(&self) -> &SourceServices { SourceServices::get(&self.ctx) }
}

/// Route tree handler methods.
impl LspProvider {
    fn lookup_companion_root(&self, ctx: &RouteCtx<'_>, name: &str) -> Node {
        let sf = source_file(ctx)?;

        // DIAGNOSTICS.md — lookup-only, hidden from readdir.
        if name == FILE_DIAGNOSTICS {
            return Ok(
                LspHandle::for_file(&self.ctx, &sf).map(|h| build_diagnostics_node(FILE_DIAGNOSTICS, &h, &self.lsp))
            );
        }

        Ok(None)
    }

    /// Lookup a file rename preview diff by name.
    fn lookup_file_rename_preview(&self, ctx: &RouteCtx<'_>, name: &str) -> Node {
        let sf = source_file(ctx)?;

        let Some(new_filename) = name.strip_suffix(".diff") else {
            return Ok(None);
        };
        let new_filename = new_filename.trim();
        if new_filename.is_empty() {
            return Ok(None);
        }

        // Only handle files this provider supports via LSP.
        if self.services().syntax.decomposer_for(&sf).is_none() {
            return Ok(None);
        }

        // Validate the new filename forms a valid path (fail-fast at lookup).
        let parent = sf.parent().unwrap_or(VfsPath::root());
        parent.join(new_filename)?;

        let Some(handle) = LspHandle::for_file(&self.ctx, &sf) else {
            return Ok(None);
        };

        let action = FileRenameDiff {
            handle,
            source_file: sf,
            new_filename: new_filename.to_owned(),
        };
        Ok(Some(DiffActionNode::into_node(name, action)))
    }

    /// Contribute LSP content nodes (CALLERS.md, DEPS.md, etc.) to a fragment directory.
    fn children_fragment_lsp(&self, ctx: &RouteCtx<'_>) -> Nodes {
        let sf = source_file(ctx)?;
        let path = ctx.params("path");

        let services = self.services();
        let Some(_decomposer) = services.syntax.decomposer_for(&sf) else {
            return Ok(None);
        };
        let shared = services.decomposition.get(&sf)?;
        let Some(frag) = find_fragment(&shared.decomposed, path) else {
            return Ok(None);
        };

        let Some(lsp_handle) = LspHandle::for_file(&self.ctx, &sf) else {
            return Ok(None);
        };

        let resolver = FragmentResolver::new(services.decomposition.clone(), sf);

        let mut nodes = build_lsp_symbol_nodes(
            &lsp_handle,
            &shared.source,
            frag.name_byte_offset,
            &self.lsp,
            &resolver,
            path,
        );

        // Code actions directory — only if the server supports it.
        if lsp_handle.capabilities().code_action_provider.is_some() {
            nodes.push(VirtualNode::directory(SUBDIR_ACTIONS));
        }

        Ok(Some(nodes))
    }

    /// Lookup LSP-specific entries in a fragment directory.
    fn lookup_fragment_lsp(&self, ctx: &RouteCtx<'_>, name: &str) -> Node {
        let sf = source_file(ctx)?;

        // rename/ is lookup-only (not in readdir) — emit bare directory
        // when LSP is available.
        if name == "rename" {
            return Ok(LspHandle::for_file(&self.ctx, &sf)
                .is_some()
                .then(|| VirtualNode::directory(name)));
        }

        // actions/ — also lookup-only as an alternative entry point.
        if name == SUBDIR_ACTIONS {
            let Some(lsp_handle) = LspHandle::for_file(&self.ctx, &sf) else {
                return Ok(None);
            };
            return Ok(lsp_handle
                .capabilities()
                .code_action_provider
                .is_some()
                .then(|| VirtualNode::directory(name)));
        }

        Ok(None)
    }

    /// Lookup a symbol rename preview diff.
    fn lookup_rename_preview(&self, ctx: &RouteCtx<'_>, name: &str) -> Node {
        let sf = source_file(ctx)?;
        let path = ctx.params("path");

        let Some(new_name) = name.strip_suffix(".diff") else {
            return Ok(None);
        };
        let new_name = new_name.trim();
        if new_name.is_empty() {
            return Ok(None);
        }

        let services = self.services();
        if services.syntax.decomposer_for(&sf).is_none() {
            return Ok(None);
        }
        let shared = services.decomposition.get(&sf)?;
        let Some(frag) = find_fragment(&shared.decomposed, path) else {
            return Ok(None);
        };

        let Some(lsp_handle) = LspHandle::for_file(&self.ctx, &sf) else {
            return Ok(None);
        };

        let query = lsp_handle.at(&shared.source, frag.name_byte_offset);
        let action = RenameDiff {
            query,
            new_name: new_name.to_owned(),
        };
        Ok(Some(DiffActionNode::into_node(name, action)))
    }

    /// List LSP code action nodes for a symbol.
    fn children_actions_dir(&self, ctx: &RouteCtx<'_>) -> Nodes {
        let path = ctx.params("path");
        lsp_links::resolve_actions_dir(&self.ctx, &source_file(ctx)?, path)
    }

    /// List LSP feature symlink nodes for a symbol.
    fn children_lsp_dir(&self, ctx: &RouteCtx<'_>) -> Nodes {
        let lsp_dir = ctx.param("lsp_dir");
        if LspFeature::from_dir_name(lsp_dir).is_none() {
            return Ok(None);
        }
        let path = ctx.params("path");
        lsp_links::resolve_lsp_symlink_dir(&self.ctx, &source_file(ctx)?, path, lsp_dir)
    }
}

/// [`Provider`] implementation for [`LspProvider`].
impl Provider for LspProvider {
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

    /// Invalidate LSP caches for changed source files.
    fn on_fs_change(&self, changed: &[VfsPath]) -> Vec<InvalidationEvent> {
        let Some(lsp) = self.ctx.get::<Arc<LspManager>>() else {
            return Vec::new();
        };
        let registry = SyntaxRegistry::global();

        changed
            .iter()
            .filter(|p| registry.decomposer_for(p).is_some())
            .filter_map(|p| {
                // Notify the LSP server: sends didChange for open documents
                // and invalidates the LSP result cache.
                let lsp_file = self.ctx.overlay_root().join(p.as_str());
                lsp.invalidate_file(&lsp_file);

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
