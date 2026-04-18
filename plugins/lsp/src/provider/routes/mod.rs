//! Extension registration and callback implementations for the LSP provider.
//!
//! Registers LSP-powered VFS nodes into [`CompanionExtensions`] (DIAGNOSTICS.md,
//! file rename) and [`SourceExtensions`] (per-feature content, fragment-level
//! directories, symbol rename) during plugin activation.

use std::sync::Arc;

use nyne::router::{CachePolicy, NamedNode, Request, RouteCtx};
use nyne_companion::{CompanionExtensions, CompanionRequest};
use nyne_diff::DiffRequest;
use nyne_source::SourceExtensions;
use strum::IntoEnumIterator;

use super::LspState;
use crate::provider::content::{Feature, actions};
use crate::session::handle::Handle;

/// Register per-file LSP contributions into the companion file extension point.
///
/// - `DIAGNOSTICS.md` — file-level diagnostics (per source file).
/// - `rename/{target}.diff` — file-level rename preview/apply.
/// - `actions/NN-*.diff` — file-wide code actions (preview/apply).
#[allow(clippy::excessive_nesting)]
pub fn register_companion_extensions(exts: &mut CompanionExtensions, state: &Arc<LspState>) {
    exts.file.scoped("lsp", |ext| {
        // DIAGNOSTICS.md at companion root.
        let s = Arc::clone(state);
        ext.content(move |_ctx, req| {
            let sf = req.source_file()?;
            let handle = Handle::for_file(&s.lsp, &sf)?;
            Some(s.diagnostics_node(&handle))
        });

        // rename/{target}.diff — file-level rename preview/apply.
        let s = Arc::clone(state);
        ext.dir(state.vfs.dir.rename.clone(), |d| {
            d.capture("target", |d| {
                d.handler(move |ctx, req, next| s.file_rename_handler(ctx, req, next));
            });
        });

        // actions/ at companion root — emit the dir entry when the LSP
        // server advertises code-action support.
        let s = Arc::clone(state);
        let actions_dir = state.vfs.dir.actions.clone();
        ext.content(move |_ctx, req| {
            let sf = req.source_file()?;
            let handle = Handle::for_file(&s.lsp, &sf)?;
            handle.capabilities().code_action_provider.as_ref()?;
            let (_, node) = NamedNode::dir(&actions_dir).into_parts();
            Some(node.with_cache_policy(CachePolicy::NoCache).named(&actions_dir))
        });

        // actions/NN-*.diff — children of the file-wide actions dir.
        let s = Arc::clone(state);
        ext.dir(state.vfs.dir.actions.clone(), |d| {
            let readdir_state = Arc::clone(&s);
            d.on_readdir(move |_ctx, req| {
                if let Some(sf) = req.source_file()
                    && let Some((resolved, _query)) = readdir_state.resolve_file_actions_dir(&sf)
                {
                    req.nodes.extend(actions::build_action_nodes(&resolved));
                }
                Ok(())
            });
            let lookup_state = Arc::clone(&s);
            d.on_lookup(move |_ctx, req, name| {
                if let Some(sf) = req.source_file()
                    && let Some((resolved, query)) = lookup_state.resolve_file_actions_dir(&sf)
                    && let Some(diff) = actions::find_action_diff(&resolved, name, &query)
                {
                    req.set_diff_source(diff, Arc::clone(&lookup_state.fs));
                }
                Ok(())
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
                        req.nodes.add(node.with_cache_policy(CachePolicy::NoCache).named(name));
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
                        for node in s.search_nodes(query) {
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
