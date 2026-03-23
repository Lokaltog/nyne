// SSOT: VFS companion name constants live in names.rs.
// Re-exported here for backward compatibility with existing imports.
use color_eyre::eyre::{Result, eyre};

use crate::dispatch::routing::ctx::RouteCtx;
use crate::node::VirtualNode;
pub use crate::types::path_conventions::{COMPANION_SUFFIX, strip_companion_suffix};
use crate::types::path_conventions::{CompanionSplit, split_companion_path};
use crate::types::real_fs::RealFs;
use crate::types::vfs_path::VfsPath;
pub mod names;
use names::SUBDIR_SYMBOLS;

/// Create a companion directory node with [`Visibility::Hidden`].
///
/// Hidden companions are filtered from normal readdir listings but
/// surfaced when `ProcessVisibility::All` is active. Use this instead
/// of `VirtualNode::directory(name)` when creating companion dir nodes.
pub fn companion_dir(name: &str) -> VirtualNode { VirtualNode::directory(name).hidden() }

/// Build the VFS path to a symbol's companion directory inside a source file's
/// companion tree.
///
/// Given `source_file = "src/lib.rs"` and `fragment_path = ["Foo", "bar"]`,
/// produces `src/lib.rs@/symbols/Foo@/bar@`.
pub fn companion_symbol_path(source_file: &VfsPath, fragment_path: &[String]) -> Result<VfsPath> {
    let parent = source_file.parent().unwrap_or(VfsPath::root());
    let name = source_file
        .name()
        .ok_or_else(|| eyre!("source file has no name: {source_file}"))?;
    let mut path = parent.join(&format!("{name}{COMPANION_SUFFIX}"))?;
    path = path.join(SUBDIR_SYMBOLS)?;
    for seg in fragment_path {
        path = path.join(&format!("{seg}{COMPANION_SUFFIX}"))?;
    }
    Ok(path)
}

/// Extract the `"source"` route param as a [`VfsPath`].
///
/// Shared helper for route handlers in companion-based providers.
pub fn source_file(ctx: &RouteCtx<'_>) -> Result<VfsPath> { VfsPath::new(ctx.param("source")) }

/// Check whether a companion split refers to an existing non-directory file.
pub fn is_file_companion(split: &CompanionSplit, real_fs: &dyn RealFs) -> bool {
    real_fs.exists(&split.source_file) && !real_fs.is_dir(&split.source_file)
}

use std::sync::Arc;

use crate::dispatch::context::RequestContext;
use crate::dispatch::routing::params::RouteParams;
use crate::dispatch::routing::tree::RouteTree;
use crate::provider::{Node, Nodes, Provider};

/// Build route params from a companion split (SSOT for source param injection).
fn companion_params(split: &CompanionSplit) -> RouteParams {
    let mut params = RouteParams::default();
    params.insert_single("source", split.source_file.as_str().to_owned());
    params
}

/// Dispatch a companion-path `children` call through a route tree.
///
/// Automatically stamps every returned node with the companion's source
/// file and current generation — providers never need to call
/// [`VirtualNode::with_source`] manually for companion nodes.
pub fn companion_children<P: Provider>(
    routes: &RouteTree<P>,
    provider: &Arc<P>,
    ctx: &RequestContext<'_>,
    split: &CompanionSplit,
) -> Nodes {
    let nodes = routes.children_at(provider, ctx, &split.rest_segments(), companion_params(split))?;
    Ok(nodes.map(|vec| stamp_companion_nodes(vec, &split.source_file, ctx)))
}

/// Dispatch a companion-path `lookup` call through a route tree.
///
/// Automatically stamps the returned node with the companion's source
/// file and current generation.
pub fn companion_lookup<P: Provider>(
    routes: &RouteTree<P>,
    provider: &Arc<P>,
    ctx: &RequestContext<'_>,
    split: &CompanionSplit,
    name: &str,
) -> Node {
    let node = routes.lookup_at(provider, ctx, &split.rest_segments(), name, companion_params(split))?;
    Ok(node.map(|n| n.with_source(split.source_file.clone(), ctx.source_generation(&split.source_file))))
}

/// Stamp all nodes with the companion source file and current generation.
fn stamp_companion_nodes(nodes: Vec<VirtualNode>, source_file: &VfsPath, ctx: &RequestContext<'_>) -> Vec<VirtualNode> {
    let generation = ctx.source_generation(source_file);
    nodes
        .into_iter()
        .map(|n| n.with_source(source_file.clone(), generation))
        .collect()
}

/// Dispatch `children` for providers with both `at_routes` and `companion_routes`.
///
/// Tries the at-routes first (for `@/` paths), then falls back to companion
/// dispatch. When `file_only` is `true`, declines companion paths that refer
/// to directories (only file companions are processed).
///
/// Eliminates the repeated `children` body in `BatchEditProvider`, `GitProvider`,
/// and similar providers that contribute to both scopes.
pub fn dispatch_children<P: Provider>(
    at_routes: &RouteTree<P>,
    companion_routes: &RouteTree<P>,
    provider: &Arc<P>,
    ctx: &RequestContext<'_>,
    file_only: bool,
) -> Nodes {
    if let Some(nodes) = at_routes.children(provider, ctx)? {
        return Ok(Some(nodes));
    }
    let Some(split) = split_companion_path(ctx.path) else {
        return Ok(None);
    };
    if file_only && !is_file_companion(&split, ctx.real_fs) {
        return Ok(None);
    }
    companion_children(companion_routes, provider, ctx, &split)
}

/// Dispatch `lookup` for providers with both `at_routes` and `companion_routes`.
///
/// Mirror of [`dispatch_children`] for single-name lookups.
pub fn dispatch_lookup<P: Provider>(
    at_routes: &RouteTree<P>,
    companion_routes: &RouteTree<P>,
    provider: &Arc<P>,
    ctx: &RequestContext<'_>,
    name: &str,
    file_only: bool,
) -> Node {
    if let Some(node) = at_routes.lookup(provider, ctx, name)? {
        return Ok(Some(node));
    }
    let Some(split) = split_companion_path(ctx.path) else {
        return Ok(None);
    };
    if file_only && !is_file_companion(&split, ctx.real_fs) {
        return Ok(None);
    }
    companion_lookup(companion_routes, provider, ctx, &split, name)
}

#[cfg(test)]
mod tests;

mod prelude;
mod util;

mod core_plugin;

mod companion;
mod directory;
mod nyne;
