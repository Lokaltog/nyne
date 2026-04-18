use std::array;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use color_eyre::eyre::{Result, eyre};
use nyne::router::{Filesystem, NamedNode, Next, NodeKind, Request, RouteCtx};
use nyne::templates::{HandleBuilder, TemplateGlobals, TemplateHandle};
use nyne_companion::CompanionRequest;
use nyne_diff::{DiffCapable, DiffRequest};
use nyne_source::{DecompositionCache, FragmentResolver, SourcePaths, SyntaxRegistry, find_fragment};
use strum::IntoEnumIterator;

use super::lsp_links;
use crate::plugin::config::vfs::Vfs;
use crate::provider::content::rename::{FileRenameDiff, RenameDiff, SymbolRename};
use crate::provider::content::{Feature, Handles, actions};
use crate::session::handle::Handle;
use crate::session::manager::Manager;

/// Shared state for LSP extension callbacks and the provider.
///
/// Built during `activate()` and shared (via `Arc`) between the extension
/// callbacks registered into [`CompanionExtensions`] / [`SourceExtensions`]
/// and the [`LspProvider`] (which only needs it for `on_change`).
pub struct LspState {
    pub(crate) lsp: Arc<Manager>,
    pub(crate) syntax: Arc<SyntaxRegistry>,
    pub(crate) decomposition: DecompositionCache,
    pub(crate) fs: Arc<dyn Filesystem>,
    pub(crate) handles: Handles,
    pub(crate) vfs: Vfs,
    pub(crate) source_paths: Arc<SourcePaths>,
}
impl LspState {
    /// Build a [`SourceCtx`] from shared services.
    pub(crate) fn source_ctx(&self) -> lsp_links::SourceCtx<'_> {
        lsp_links::SourceCtx {
            syntax: &self.syntax,
            decomposition: &self.decomposition,
            symbols_dir: self.source_paths.symbols_dir(),
        }
    }
}
/// Build all template handles for LSP features and diagnostics.
///
/// Registers LSP-specific template globals from the VFS config (replacing
/// the former compile-time constants), shared partials, per-feature
/// templates, and the file-level diagnostics template.
pub fn build_handles(vfs: &Vfs) -> Handles {
    let mut b = HandleBuilder::new();

    // LSP-specific globals from VFS config.
    vfs.register_globals(b.engine_mut());

    // Shared partials (included by individual LSP templates).
    b.register_partial(
        "syntax/lsp/_locations",
        include_str!("../templates/lsp/_locations.md.j2"),
    );

    // Register file name globals so templates can reference e.g. FILE_DEFINITION.
    Feature::register_globals(b.engine_mut());

    // Per-feature LSP templates — order derived from Feature::iter().
    let lsp_keys: Vec<_> = Feature::iter()
        .map(|f| {
            let (name, src) = f.template();
            b.register(name, src)
        })
        .collect();

    // File-level diagnostics template.
    let diagnostics_key = b.register(
        "syntax/lsp/diagnostics",
        include_str!("../templates/lsp/diagnostics.md.j2"),
    );

    let engine = b.finish();
    let mut lsp_keys = lsp_keys.into_iter();

    Handles {
        #[expect(clippy::expect_used, reason = "length matches Feature::COUNT by construction")]
        features: array::from_fn(|_| TemplateHandle::new(&engine, lsp_keys.next().expect("Feature::COUNT mismatch"))),
        diagnostics: TemplateHandle::new(&engine, diagnostics_key),
    }
}

/// Result of resolving a fragment path with optional sub-route.
struct ResolvedFragment<'a> {
    /// The source file backing this fragment.
    source_file: PathBuf,
    /// Path segments identifying the fragment within the decomposition.
    frag_segments: &'a [String],
    /// Optional sub-route (e.g. "actions", "rename") split off from the path.
    sub_route: Option<&'a str>,
}

/// Extension callback implementations on [`LspState`].
///
/// These methods are called from the closures registered via
/// [`register_companion_extensions`](super::routes::register_companion_extensions)
/// and [`register_source_extensions`](super::routes::register_source_extensions).
/// Each closure captures an `Arc<LspState>` and delegates to a method here.
impl LspState {
    /// Content producer for a single LSP feature (CALLERS.md, DEPS.md, etc.).
    ///
    /// Returns the template node for `feature` if the fragment resolves,
    /// the LSP server is available, and the feature is supported.
    pub(crate) fn feature_content(&self, ctx: &RouteCtx, req: &Request, feature: Feature) -> Option<NamedNode> {
        let sf = req.source_file()?;
        let segments: Vec<String> = ctx.param("path")?.split('/').map(String::from).collect();

        let shared = self.decomposition.get(&sf).ok()?;
        let frag = find_fragment(&shared.decomposed, &segments)?;

        let lsp_handle = Handle::for_file(&self.lsp, &sf)?;
        if !feature.is_supported(lsp_handle.capabilities()) {
            return None;
        }
        let resolver = FragmentResolver::new(self.decomposition.clone(), sf);
        let fragment_path: Arc<[String]> = Arc::from(segments);
        let source = shared.source.clone();
        let name_byte_offset = frag.span.name_byte_offset;
        Some(
            self.handles
                .features
                .get(feature.handle_index())?
                .lazy_node(feature.file_name(), move |engine, tmpl| {
                    let slr = resolver
                        .line_range(&fragment_path)?
                        .ok_or_else(|| eyre!("symbol no longer exists in source"))?;
                    let line_range = (slr.start - 1)..slr.end;
                    let query = lsp_handle
                        .over_lines(line_range)
                        .with_position(&source, name_byte_offset);
                    let result = feature.query(&query)?;
                    Ok(result.render_view(engine, tmpl, query.path_resolver()))
                }),
        )
    }

    /// Handler for `rename/{target}` — file-level rename preview on read, apply on unlink.
    pub(crate) fn file_rename_handler(&self, ctx: &RouteCtx, req: &mut Request, next: &Next) -> Result<()> {
        let Some(sf) = req.source_file() else {
            return next.run(req);
        };
        let Some(target_name) = ctx.param("target") else {
            return next.run(req);
        };

        if !req.op().is_readdir() && req.op().lookup_name().is_none() {
            return next.run(req);
        }

        let Some(new_filename) = target_name
            .strip_suffix(".diff")
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            return next.run(req);
        };

        if self.syntax.decomposer_for(&sf).is_none() {
            return next.run(req);
        }

        let Some(handle) = Handle::for_file(&self.lsp, &sf) else {
            return next.run(req);
        };

        req.set_diff_source(
            FileRenameDiff {
                handle,
                source_file: sf,
                new_filename: new_filename.to_owned(),
            },
            Arc::clone(&self.fs),
        );
        next.run(req)
    }

    /// Fragment readdir callback — contributes LSP directories and handles sub-routes.
    ///
    /// Registered as an `on_readdir` callback on [`SourceExtensions::fragment_path`].
    #[allow(clippy::unnecessary_wraps, clippy::excessive_nesting)]
    /// Fires after source's own readdir callback, so source-contributed directory
    /// nodes are already present in `req.nodes` (used by [`attach_renameables`]).
    ///
    /// Sub-routes: if the last path segment is a known LSP directory (actions/,
    /// callers/, etc.), dispatches to the appropriate sub-route handler instead
    /// of contributing fragment-level children.
    pub(crate) fn fragment_readdir(&self, ctx: &RouteCtx, req: &mut Request) -> Result<()> {
        let Some(path_param) = ctx.param("path") else {
            return Ok(());
        };
        let segments: Vec<String> = path_param.split('/').map(String::from).collect();
        let Some(resolved) = self.resolve_fragment(req, &segments) else {
            return Ok(());
        };

        let source_ctx = self.source_ctx();

        if let Some(lsp_dir) = resolved.sub_route {
            let nodes = if lsp_dir == self.vfs.dir.actions {
                lsp_links::resolve_actions_dir(&source_ctx, &self.lsp, &resolved.source_file, resolved.frag_segments)
                    .ok()
                    .flatten()
                    .map(|(resolved_actions, _query)| actions::build_action_nodes(&resolved_actions))
            } else {
                req.companion().cloned().and_then(|c| {
                    lsp_links::resolve_lsp_symlink_dir(
                        &c,
                        &source_ctx,
                        &self.lsp,
                        &resolved.source_file,
                        resolved.frag_segments,
                        lsp_dir,
                    )
                    .ok()
                    .flatten()
                })
            };
            if let Some(nodes) = nodes {
                req.nodes.extend(nodes);
            }
        } else {
            self.contribute_fragment_children(req, &resolved.source_file);
            self.attach_renameables(req, &resolved.source_file);
        }
        Ok(())
    }

    /// Fragment lookup callback — handles LSP-specific entries and sub-routes.
    ///
    /// Registered as an `on_lookup` callback on [`SourceExtensions::fragment_path`].
    #[allow(clippy::unnecessary_wraps)]
    pub(crate) fn fragment_lookup(&self, ctx: &RouteCtx, req: &mut Request, name: &str) -> Result<()> {
        let Some(path_param) = ctx.param("path") else {
            return Ok(());
        };
        let segments: Vec<String> = path_param.split('/').map(String::from).collect();
        let Some(resolved) = self.resolve_fragment(req, &segments) else {
            return Ok(());
        };

        let source_ctx = self.source_ctx();

        match resolved.sub_route {
            // Sub-route: actions/ lookup/remove — set DiffCapable for the diff middleware.
            Some(actions_dir) if actions_dir == self.vfs.dir.actions => {
                if let Ok(Some((resolved_actions, query))) = lsp_links::resolve_actions_dir(
                    &source_ctx,
                    &self.lsp,
                    &resolved.source_file,
                    resolved.frag_segments,
                ) && let Some(diff) = actions::find_action_diff(&resolved_actions, name, &query)
                {
                    req.set_diff_source(diff, Arc::clone(&self.fs));
                }
            }
            // Sub-route: rename/ lookup/remove — set DiffCapable for the diff middleware.
            Some(rename) if rename == self.vfs.dir.rename => {
                if let Some(new_name) = name.strip_suffix(".diff").map(str::trim).filter(|s| !s.is_empty()) {
                    self.set_rename_diff_source(req, &resolved.source_file, resolved.frag_segments, new_name);
                }
            }
            // Sub-route: {lsp_dir}/ lookup — resolve symlink by name.
            Some(lsp_dir) =>
                if let Some(companion) = req.companion().cloned()
                    && let Ok(Some(nodes)) = lsp_links::resolve_lsp_symlink_dir(
                        &companion,
                        &source_ctx,
                        &self.lsp,
                        &resolved.source_file,
                        resolved.frag_segments,
                        lsp_dir,
                    )
                    && let Some(node) = nodes.into_iter().find(|n| n.name() == name)
                {
                    req.nodes.add(node);
                },
            // Fragment-level lookup.
            None => {
                self.handle_fragment_lookup(req, &resolved.source_file, &segments, name);
                self.attach_renameables(req, &resolved.source_file);
            }
        }
        Ok(())
    }

    /// Validate fragment context and split off any sub-route.
    ///
    /// Shared preamble for [`fragment_readdir`](Self::fragment_readdir) and
    /// [`fragment_lookup`](Self::fragment_lookup): resolves the source file,
    /// detects sub-routes, and verifies the fragment exists.
    /// Returns `None` if any guard fails.
    fn resolve_fragment<'a>(&self, req: &Request, segments: &'a [String]) -> Option<ResolvedFragment<'a>> {
        let sf = req.source_file()?;
        let (frag_segments, sub_route) = split_sub_route(segments, &self.vfs.dir.actions, &self.vfs.dir.rename);
        self.decomposition
            .get(&sf)
            .ok()
            .filter(|shared| find_fragment(&shared.decomposed, frag_segments).is_some())?;
        Some(ResolvedFragment {
            source_file: sf,
            frag_segments,
            sub_route,
        })
    }

    /// Contribute LSP directories to a fragment directory.
    ///
    /// Emits `actions/` and per-feature symlink directories (callers/, deps/,
    /// references/, etc.) based on server capabilities. `rename/` is
    /// lookup-only (requires the target name) and not listed here.
    fn contribute_fragment_children(&self, req: &mut Request, sf: &Path) {
        let Some(handle) = Handle::for_file(&self.lsp, sf) else {
            return;
        };
        let caps = handle.capabilities();

        if caps.code_action_provider.is_some() {
            req.nodes.add(NamedNode::dir(&self.vfs.dir.actions));
        }

        for feature in Feature::iter() {
            if feature.is_supported(caps)
                && let Some(dir) = feature.dir_name()
            {
                req.nodes.add(NamedNode::dir(dir));
            }
        }
    }

    /// Handle lookup for LSP-specific entries in a fragment directory.
    fn handle_fragment_lookup(&self, req: &mut Request, sf: &Path, segments: &[String], name: &str) {
        // rename/ — lookup-only (not in readdir).
        if name == self.vfs.dir.rename {
            if Handle::for_file(&self.lsp, sf).is_some() {
                req.nodes.add(NamedNode::dir(name));
            }
            return;
        }

        // actions/ — lookup-only as an alternative entry point.
        if name == self.vfs.dir.actions {
            if Handle::for_file(&self.lsp, sf).is_some_and(|h| h.capabilities().code_action_provider.is_some()) {
                req.nodes.add(NamedNode::dir(name));
            }
            return;
        }

        // Symbol rename preview: {new_name}.diff — set DiffCapable for the diff middleware.
        // Only if no upstream callback already claimed this path (e.g. source's delete.diff).
        if req.state::<DiffCapable>().is_none()
            && let Some(new_name) = name.strip_suffix(".diff").map(str::trim).filter(|s| !s.is_empty())
        {
            self.set_rename_diff_source(req, sf, segments, new_name);
            return;
        }

        // LSP feature symlink directories — confirm existence on lookup.
        if let Some(feature) = Feature::from_dir_name(name)
            && Handle::for_file(&self.lsp, sf).is_some_and(|h| feature.is_supported(h.capabilities()))
        {
            req.nodes.add(NamedNode::dir(name));
        }
    }

    /// Set [`DiffCapable`] state for a symbol rename preview.
    fn set_rename_diff_source(&self, req: &mut Request, sf: &Path, segments: &[String], new_name: &str) {
        let Some(shared) = self
            .syntax
            .decomposer_for(sf)
            .and_then(|_| self.decomposition.get(sf).ok())
        else {
            return;
        };
        let Some(frag) = find_fragment(&shared.decomposed, segments) else {
            return;
        };
        let Some(lsp_handle) = Handle::for_file(&self.lsp, sf) else {
            return;
        };

        req.set_diff_source(
            RenameDiff {
                query: lsp_handle.at(&shared.source, frag.span.name_byte_offset),
                new_name: new_name.to_owned(),
            },
            Arc::clone(&self.fs),
        );
    }

    /// Post-readdir/lookup: attach `Renameable` to directory nodes that don't already have one.
    ///
    /// Runs inside the `on_readdir` / `on_lookup` extension callback, after source's
    /// own readdir/lookup has fired. Source contributes the fragment directory nodes
    /// (e.g. `Foo@/`, `Bar@/`); this method attaches LSP rename capability to them.
    fn attach_renameables(&self, req: &mut Request, sf: &Path) {
        let Some(lsp_handle) = Handle::for_file(&self.lsp, sf) else {
            return;
        };
        let Ok(shared) = self.decomposition.get(sf) else {
            return;
        };

        let Some(companion) = req.companion().cloned() else {
            return;
        };

        for node in req.nodes.iter_mut() {
            if node.kind() != NodeKind::Directory || node.renameable().is_some() {
                continue;
            }
            let node_name = node.name().to_owned();
            let Some(bare_name) = companion.strip_suffix(&node_name) else {
                continue;
            };
            let path = [bare_name.to_owned()];
            let Some(frag) = find_fragment(&shared.decomposed, &path) else {
                continue;
            };
            let query = lsp_handle.at(&shared.source, frag.span.name_byte_offset);
            node.set_renameable(SymbolRename {
                query,
                fs: Arc::clone(&self.fs),
            });
        }
    }
}

/// Split off a sub-route from the fragment path.
///
/// If the last segment is a known LSP directory (actions, rename, or a feature
/// dir), returns the fragment segments and the sub-route name. Otherwise
/// returns the full segments with no sub-route.
pub fn split_sub_route<'a>(
    segments: &'a [String],
    actions_dir: &str,
    rename_dir: &str,
) -> (&'a [String], Option<&'a str>) {
    let is_sub_route = |name: &str| name == actions_dir || name == rename_dir || Feature::from_dir_name(name).is_some();
    match segments.split_last() {
        Some((last, init)) if is_sub_route(last) => (init, Some(last.as_str())),
        _ => (segments, None),
    }
}

#[cfg(test)]
mod tests;
