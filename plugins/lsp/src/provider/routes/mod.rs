//! Extension registration and callback implementations for the LSP provider.
//!
//! Registers LSP-powered VFS nodes into [`CompanionExtensions`] (DIAGNOSTICS.md,
//! file rename) and [`SourceExtensions`] (per-feature content, fragment-level
//! directories, symbol rename) during plugin activation.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use nyne::router::{CachePolicy, NamedNode, Request, RouteCtx};
use nyne_companion::{CompanionExtensions, CompanionRequest};
use nyne_source::SourceExtensions;
use strum::IntoEnumIterator;

use super::{LspState, lsp_links};
use crate::provider::content::{Feature, build_diagnostics_node};
use crate::session::handle::Handle;

/// Register per-file LSP contributions into the companion file extension point.
///
/// - `DIAGNOSTICS.md` — file-level diagnostics (per source file).
/// - `rename/{target}.diff` — file-level rename preview/apply.
pub fn register_companion_extensions(exts: &mut CompanionExtensions, state: &Arc<LspState>) {
    exts.file.scoped("lsp", |ext| {
        // DIAGNOSTICS.md at companion root.
        let s = Arc::clone(state);
        ext.content(move |_ctx, req| {
            let sf = req.source_file()?;
            let handle = Handle::for_file(&s.lsp, &sf)?;
            Some(build_diagnostics_node(&s.vfs.file.diagnostics, &handle, &s.handles))
        });

        // rename/{target}.diff — file-level rename preview/apply.
        let s = Arc::clone(state);
        ext.dir(state.vfs.dir.rename.clone(), |d| {
            d.capture("target", |d| {
                d.handler(move |ctx, req, next| s.file_rename_handler(ctx, req, next));
            });
        });
    });
}
/// Register mount-wide LSP contributions into the companion mount extension point.
///
/// - `search/symbols/{query}/` — workspace symbol search with symlinks to matches.
#[allow(clippy::excessive_nesting)]
pub fn register_mount_extensions(exts: &mut CompanionExtensions, state: &Arc<LspState>) {
    let search_dir = state.vfs.dir.search.clone();
    exts.mount.scoped("lsp", |ext| {
        let s = Arc::clone(state);
        ext.dir(search_dir, |d| {
            d.dir("symbols", |d| {
                // Validate query at lookup — emit a dir node if the LSP
                // server returns any workspace symbols for this name.
                let s2 = Arc::clone(&s);
                d.on_lookup(move |_ctx: &RouteCtx, req: &mut Request, name: &str| {
                    if !s2.lsp.workspace_symbols(name).is_empty() {
                        let (_, node) = NamedNode::dir(name).into_parts();
                        req.nodes.add(
                            node.with_cache_policy(CachePolicy::with_ttl(Duration::ZERO))
                                .named(name),
                        );
                    }
                    Ok(())
                });

                // {query}/ — readdir populates symlinks to matching symbols.
                // Child lookups use the readdir fallback (no on_lookup here),
                // which retains the correct symlink node type.
                let s = Arc::clone(&s);
                d.capture("query", |d| {
                    d.on_readdir(move |ctx: &RouteCtx, req: &mut Request| {
                        let Some(query) = ctx.param("query") else {
                            return Ok(());
                        };
                        let symbols = s.lsp.workspace_symbols(query);
                        let base = PathBuf::from(format!("@/{}/symbols/{query}", s.vfs.dir.search));
                        for node in lsp_links::build_search_symlinks(
                            &symbols,
                            s.lsp.path_resolver().source_root(),
                            &base,
                            &s.source_paths,
                        ) {
                            req.nodes.add(node);
                        }
                        Ok(())
                    });
                });
            });
        });
    });
}

/// Register LSP contributions inside fragment directories (`symbols/{..path}`).
///
/// - Per-feature content (CALLERS.md, DEPS.md, REFERENCES.md, etc.).
/// - Fragment-level readdir (actions/, callers/, deps/, etc.).
/// - Fragment-level lookup (rename preview, feature dirs).
pub fn register_source_extensions(exts: &mut SourceExtensions, state: &Arc<LspState>) {
    exts.fragment_path.scoped("lsp", |ext| {
        // Per-feature content (CALLERS.md, DEPS.md, etc.).
        for feature in Feature::iter() {
            let s = Arc::clone(state);
            ext.content(move |ctx, req| s.feature_content(ctx, req, feature));
        }

        // Fragment readdir — contribute LSP dirs and handle sub-routes.
        let s = Arc::clone(state);
        ext.on_readdir(move |ctx, req| s.fragment_readdir(ctx, req));

        // Fragment lookup — handle LSP-specific entries and sub-routes.
        let s = Arc::clone(state);
        ext.on_lookup(move |ctx, req, name| s.fragment_lookup(ctx, req, name));
    });
}
