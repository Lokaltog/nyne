//! Workspace symbol search provider — exposes `@/search/symbols/{query}`.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use lsp_types::SymbolInformation;
use nyne::dispatch::activation::ActivationContext;
use nyne::dispatch::context::RequestContext;
use nyne::dispatch::routing::ctx::RouteCtx;
use nyne::dispatch::routing::tree::RouteTree;
use nyne::node::VirtualNode;
use nyne::provider::{Node, Nodes, Provider, ProviderId};
use nyne::types::vfs_path::VfsPath;

use crate::config::CodingConfig;
use crate::lsp::manager::LspManager;
use crate::lsp::uri::uri_to_file_path;
use crate::providers::names::{SUBDIR_AT_LINE, SUBDIR_SYMBOLS};

/// Workspace symbol search provider.
///
/// Contributes `@/search/symbols/{query}` — a directory of symlinks
/// pointing to matching symbols in the VFS. Empty directory if no match.
pub(crate) struct WorkspaceSearchProvider {
    ctx: Arc<ActivationContext>,
    routes: RouteTree<Self>,
}

/// Methods for [`WorkspaceSearchProvider`].
impl WorkspaceSearchProvider {
    /// Unique provider identifier for workspace search.
    pub(crate) const PROVIDER_ID: ProviderId = ProviderId::new("workspace-search");

    /// Create a new workspace search provider with route tree.
    pub(crate) fn new(ctx: Arc<ActivationContext>) -> Self {
        let routes = nyne_macros::routes!(Self, {
            no_emit "@" {
                "search" {
                    "symbols" {
                        lookup(lookup_query),
                        "{query}" => children_results,
                    }
                }
            }
        });

        Self { ctx, routes }
    }

    /// Query the LSP for workspace symbols matching a string.
    ///
    /// Returns the raw LSP results, or an empty vec if no LSP is available.
    /// Extracted as a dedicated method so callers can reuse it (and a cache
    /// layer can be inserted here in the future without changing call sites).
    fn query_symbols(&self, query: &str) -> Vec<SymbolInformation> {
        let Some(lsp_manager) = self.ctx.get::<Arc<LspManager>>() else {
            return Vec::new();
        };
        lsp_manager.workspace_symbols(query)
    }

    /// Return a directory for a query string, if LSP has matching symbols.
    ///
    /// Returns `None` (ENOENT) when no symbols match. The directory is
    /// marked `no_cache` so the kernel re-lookups every time and the
    /// dispatch layer re-resolves its contents on every access —
    /// workspace search results depend on external LSP state.
    fn lookup_query(&self, _ctx: &RouteCtx<'_>, name: &str) -> Option<VirtualNode> {
        if self.query_symbols(name).is_empty() {
            return None;
        }
        Some(VirtualNode::directory(name).no_cache())
    }

    /// Build symlinks for workspace symbols matching the captured query.
    fn children_results(&self, ctx: &RouteCtx<'_>) -> Vec<VirtualNode> {
        let query = ctx.param("query");
        let symbols = self.query_symbols(query);

        let Ok(base) = VfsPath::new("@/search/symbols").and_then(|p| p.join(query)) else {
            return Vec::new();
        };

        build_symlinks(&symbols, self.ctx.overlay_root(), &base)
    }
}

/// [`Provider`] implementation for [`WorkspaceSearchProvider`].
impl Provider for WorkspaceSearchProvider {
    /// Return the workspace search provider identifier.
    fn id(&self) -> ProviderId { Self::PROVIDER_ID }

    /// Activate when LSP workspace symbol search is available.
    fn should_activate(&self, ctx: &ActivationContext) -> bool {
        ctx.get::<CodingConfig>().is_some_and(|c| c.lsp.enabled)
    }

    /// Dispatch children through the route tree.
    fn children(self: Arc<Self>, ctx: &RequestContext<'_>) -> Nodes { self.routes.children(&self, ctx) }

    /// Dispatch lookup through the route tree.
    fn lookup(self: Arc<Self>, ctx: &RequestContext<'_>, name: &str) -> Node { self.routes.lookup(&self, ctx, name) }
}

/// Convert LSP symbol results into VFS symlinks.
///
/// Each symlink targets `<file>@/symbols/at-line/<line>`, which the
/// VFS resolves to the containing symbol's body. Link names use the
/// file basename to avoid path separators in filenames.
fn build_symlinks(symbols: &[SymbolInformation], overlay_root: &Path, base: &VfsPath) -> Vec<VirtualNode> {
    let mut nodes = Vec::new();
    let mut seen = HashSet::new();

    for sym in symbols {
        let abs_path = uri_to_file_path(&sym.location.uri);
        let Some(rel_path) = abs_path.strip_prefix(overlay_root).ok() else {
            continue;
        };
        let Some(rel_str) = rel_path.to_str() else {
            continue;
        };

        // LSP lines are 0-based; at-line uses 1-based.
        let line = sym.location.range.start.line + 1;

        // Target: <file>@/symbols/at-line/<line>
        let Ok(target) = VfsPath::new(&format!("{rel_str}@/{SUBDIR_SYMBOLS}/{SUBDIR_AT_LINE}/{line}")) else {
            continue;
        };

        // Link name: <basename>::<symbol_name> (no slashes)
        let file = rel_path.file_name().and_then(|n| n.to_str()).unwrap_or(rel_str);
        let link_name = [file, "::", &sym.name].concat();

        // Deduplicate by link name — first occurrence wins.
        if seen.insert(link_name.clone()) {
            nodes.push(VirtualNode::symlink(link_name, target.relative_to(base)).no_cache());
        }
    }

    nodes
}

/// Unit tests.
#[cfg(test)]
mod tests;
