//! LSP feature nodes, diagnostics, and view rendering.
//!
//! Bridges the LSP client with the VFS by turning LSP responses (hover, references,
//! callers, definitions, etc.) into readable virtual files and symlink directories.
//! [`Feature`] is the single source of truth for supported features — adding a
//! new one requires only a variant there plus a Jinja template.
//!
//! Architecture:
//!   - **resolve time** — `Handle::for_file` gates on LSP availability,
//!     `Handle::at` pre-computes the LSP position.
//!   - **read time** — `TemplateView` impls acquire a `FileQuery`, execute
//!     the cached LSP call, and render via template.
//!   - **symlink dirs** — emitted as `NamedNode::dir` at resolve time,
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
use std::sync::Arc;

use color_eyre::eyre::{Result, eyre};
pub use feature::{Feature, Handles, Target};
use nyne::router::NamedNode;

use crate::session::diagnostic_view::diagnostics_to_rows;

/// Error message when the LSP client has become unavailable since resolve time.
pub const LSP_UNAVAILABLE: &str = "LSP server no longer available";

use std::time::Duration;

use nyne::router::CachePolicy;

use crate::session::handle::Handle;

/// Build the file-level DIAGNOSTICS.md node.
///
/// Uses `no_cache()` because diagnostics depend on external LSP state
/// that changes asynchronously — the `DiagnosticStore::get_or_wait`
/// freshness gate in the read pipeline handles blocking until the LSP
/// publishes fresh results after a `didChange`.
pub fn build_diagnostics_node(name: &str, handle: &Arc<Handle>, lsp_handles: &Handles) -> NamedNode {
    let handle = Arc::clone(handle);
    let (name, node) = lsp_handles
        .diagnostics
        .lazy_node(name, move |engine, tmpl| {
            let fq = handle.file_query().ok_or_else(|| eyre!(LSP_UNAVAILABLE))?;
            let diags = fq.diagnostics()?;
            let items = diagnostics_to_rows(&diags);
            Ok(engine.render_bytes(tmpl, &views::DiagnosticsView { items: &items }))
        })
        .into_parts();
    node.with_cache_policy(CachePolicy::with_ttl(Duration::ZERO))
        .named(name)
}

/// Query the LSP for symlink directory targets.
///
/// Looks up the feature by directory name and delegates to
/// `Feature::query()` → `QueryResult::into_targets()`.
pub fn query_lsp_targets(
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
