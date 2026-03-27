//! LSP feature nodes, diagnostics, and view rendering.
//!
//! Bridges the LSP client with the VFS by turning LSP responses (hover, references,
//! callers, definitions, etc.) into readable virtual files and symlink directories.
//! [`Feature`] is the single source of truth for supported features — adding a
//! new one requires only a variant there plus a Jinja template.
//!
//! Architecture:
//!   - **resolve time** — `LspHandle::for_file` gates on LSP availability,
//!     `LspHandle::at` pre-computes the LSP position.
//!   - **read time** — `TemplateView` impls acquire a `FileQuery`, execute
//!     the cached LSP call, and render via template.
//!   - **symlink dirs** — emitted as `VirtualNode::directory` at resolve time,
//!     populated with symlinks when the directory itself is resolved (lazy reverse-map).

/// Code action resolution and node construction.
pub mod actions;
/// LSP feature definitions and query dispatch.
pub mod feature;
/// Formatting helpers for LSP data types.
mod format;
/// LSP-powered rename support.
pub mod rename;
/// View types for rendering LSP query results.
mod views;

use std::ops::Range as StdRange;

use color_eyre::eyre::eyre;
pub(crate) use feature::{Feature, Handles, Target};
use nyne::prelude::*;
use nyne_source::providers::fragment_resolver::FragmentResolver;
use strum::{EnumCount, IntoEnumIterator};
use views::{DiagnosticsLspView, SymbolLspView};

/// Error message when the LSP client has become unavailable since resolve time.
pub const LSP_UNAVAILABLE: &str = "LSP server no longer available";

use crate::session::handle::Handle;

/// Build LSP-backed virtual file nodes for a single symbol.
///
/// Iterates all [`Feature`] variants, skipping features the server
/// does not support (based on advertised capabilities). Adding a new
/// LSP feature only requires adding a variant to `Feature` — no
/// changes here.
pub(crate) fn build_lsp_symbol_nodes(
    handle: &Arc<Handle>,
    source: &str,
    name_byte_offset: usize,
    lsp_handles: &Handles,
    resolver: &FragmentResolver,
    fragment_path: &Arc<[String]>,
) -> Vec<VirtualNode> {
    let sym = handle.at(source, name_byte_offset);
    let caps = handle.capabilities();
    let mut nodes = Vec::with_capacity(Feature::COUNT * 2);

    for feature in Feature::iter() {
        if !feature.is_supported(caps) {
            continue;
        }
        let Some(tmpl) = lsp_handles.features.get(feature.handle_index()) else {
            continue;
        };
        nodes.push(tmpl.node(feature.file_name(), SymbolLspView {
            query: sym.clone(),
            feature,
            resolver: resolver.clone(),
            fragment_path: Arc::clone(fragment_path),
        }));
        if let Some(dir) = feature.dir_name() {
            nodes.push(VirtualNode::directory(dir));
        }
    }

    nodes
}

/// Build the file-level DIAGNOSTICS.md node.
///
/// Uses `no_cache()` because diagnostics depend on external LSP state
/// that changes asynchronously — the `DiagnosticStore::get_or_wait`
/// freshness gate in the read pipeline handles blocking until the LSP
/// publishes fresh results after a `didChange`.
pub(crate) fn build_diagnostics_node(name: &str, handle: &Arc<Handle>, lsp_handles: &Handles) -> VirtualNode {
    lsp_handles
        .diagnostics
        .node(name, DiagnosticsLspView(Arc::clone(handle)))
        .no_cache()
}

/// Query the LSP for symlink directory targets.
///
/// Looks up the feature by directory name and delegates to
/// `LspFeature::query()` → `LspQueryResult::into_targets()`.
pub(crate) fn query_lsp_targets(
    handle: &Arc<Handle>,
    source: &str,
    name_byte_offset: usize,
    lsp_dir: &str,
    line_range: &StdRange<usize>,
) -> Result<Vec<Target>> {
    let Some(feature) = Feature::from_dir_name(lsp_dir) else {
        return Ok(Vec::new());
    };
    let sym = handle.at(source, name_byte_offset);
    let fq = sym.file_query().ok_or_else(|| eyre!(LSP_UNAVAILABLE))?;
    Ok(feature
        .query(&fq, sym.position(), line_range)?
        .into_targets(handle.path_resolver()))
}
