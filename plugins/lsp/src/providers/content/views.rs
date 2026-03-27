//! View types for rendering LSP query results into Jinja templates.
//!
//! Each LSP feature has a corresponding view struct that transforms raw LSP
//! protocol responses into serializable data for template rendering.
//! [`QueryResult`] is the unified return type from all LSP queries.

use std::path::PathBuf;

use color_eyre::eyre::eyre;
use lsp_types::{CallHierarchyItem, Hover, InlayHint, Location, Position};
use nyne::prelude::*;
use nyne::templates::TemplateEngine;
use nyne_source::providers::fragment_resolver::FragmentResolver;
use serde::Serialize;

use super::feature::{Feature, Target};
use super::format::{extract_hover_content, extract_inlay_label};
use crate::session::diagnostic_view::{DiagnosticRow, diagnostics_to_rows};
use crate::session::handle::{Handle, SymbolQuery};
use crate::session::path::PathResolver;
use crate::session::uri::uri_to_file_path;

/// Per-symbol LSP view — acquires a `FileQuery` at read time and
/// delegates to `LspFeature::query()` + `LspQueryResult::render_view()`.
///
/// Replaces the previous 5 separate `TemplateView` impls with one.
pub(super) struct SymbolLspView {
    pub query: SymbolQuery,
    pub feature: Feature,
    pub resolver: FragmentResolver,
    pub fragment_path: Arc<[String]>,
}

/// [`TemplateView`] implementation for [`SymbolLspView`].
impl TemplateView for SymbolLspView {
    /// Acquire a file query and render the symbol-scoped LSP view.
    fn render(&self, engine: &TemplateEngine, template: &str) -> Result<Vec<u8>> {
        let fq = self.query.file_query().ok_or_else(|| eyre!(super::LSP_UNAVAILABLE))?;
        let path_resolver = self.query.path_resolver();
        let slr = self
            .resolver
            .line_range(&self.fragment_path)?
            .ok_or_else(|| eyre!("symbol no longer exists in source"))?;
        let line_range = (slr.start - 1)..slr.end;
        let result = self.feature.query(&fq, self.query.position(), &line_range)?;
        Ok(result.render_view(engine, template, path_resolver))
    }
}

/// File-level diagnostics view — not position-scoped.
pub(super) struct DiagnosticsLspView(pub Arc<Handle>);

/// [`TemplateView`] implementation for [`DiagnosticsLspView`].
impl TemplateView for DiagnosticsLspView {
    /// Render file-level diagnostics via template.
    fn render(&self, engine: &TemplateEngine, template: &str) -> Result<Vec<u8>> {
        let fq = self.0.file_query().ok_or_else(|| eyre!(super::LSP_UNAVAILABLE))?;
        let diags = fq.diagnostics()?;
        let items = diagnostics_to_rows(&diags);

        Ok(engine.render_bytes(template, &DiagnosticsView { items: &items }))
    }
}

/// Shared view for location-list results (references, definition, etc.).
#[derive(Serialize)]
pub(super) struct LocationsView {
    locations: Vec<LocationRow>,
}

/// A location row for LSP location-list results.
#[derive(Serialize)]
struct LocationRow {
    file: String,
    line: u32,
    col: u32,
}

/// Methods for [`LocationsView`].
impl LocationsView {
    /// Build a locations view from raw LSP locations.
    pub(super) fn from_locations(locs: &[Location], resolver: &PathResolver) -> Self {
        Self {
            locations: locs
                .iter()
                .map(|loc| LocationRow {
                    file: resolver
                        .rewrite_to_fuse(&uri_to_file_path(&loc.uri))
                        .display()
                        .to_string(),
                    line: loc.range.start.line + 1,
                    col: loc.range.start.character + 1,
                })
                .collect(),
        }
    }
}

/// Rendered hover content for display.
#[derive(Serialize)]
pub(super) struct HoverView {
    content: String,
}

/// Methods for [`HoverView`].
impl HoverView {
    /// Create a hover view from an optional LSP hover response.
    pub(super) fn new(hover: Option<&Hover>) -> Self {
        Self {
            content: hover.map(|h| extract_hover_content(&h.contents)).unwrap_or_default(),
        }
    }
}

/// Unified view for hierarchy results (callers, deps).
///
/// Templates iterate `items` with `name`/`file`/`line`.
#[derive(Serialize)]
pub(super) struct HierarchyListView<'a> {
    pub items: &'a [HierarchyRow],
}

/// Row for call hierarchy results (callers, deps).
#[derive(Clone, Serialize)]
pub(super) struct HierarchyRow {
    pub name: String,
    pub kind: &'static str,
    pub file: PathBuf,
    pub line: u32,
}

/// Extract a `HierarchyRow` from a `CallHierarchyItem`.
pub(super) fn hierarchy_item(item: CallHierarchyItem) -> HierarchyRow {
    HierarchyRow {
        name: item.name,
        kind: "",
        file: uri_to_file_path(&item.uri),
        line: item.selection_range.start.line + 1,
    }
}

/// View for inlay hints rendering.
#[derive(Serialize)]
pub(super) struct InlayHintsView {
    hints: Vec<InlayHintRow>,
}

/// An inlay hint row for template rendering.
#[derive(Serialize)]
struct InlayHintRow {
    line: u32,
    col: u32,
    label: String,
    kind: &'static str,
}

/// Methods for [`InlayHintsView`].
impl InlayHintsView {
    /// Build an inlay hints view from raw LSP hints.
    pub(super) fn from_hints(raw: &[InlayHint]) -> Self {
        Self {
            hints: raw
                .iter()
                .map(|h| {
                    let Position { line, character } = h.position;
                    InlayHintRow {
                        line: line + 1,
                        col: character + 1,
                        label: extract_inlay_label(&h.label),
                        kind: h.kind.map_or("unknown", |k| match k {
                            lsp_types::InlayHintKind::TYPE => "type",
                            lsp_types::InlayHintKind::PARAMETER => "parameter",
                            _ => "other",
                        }),
                    }
                })
                .collect(),
        }
    }
}

/// View for rendering file-level LSP diagnostics (errors, warnings, hints).
///
/// Unlike per-symbol views, diagnostics cover the entire file and are not
/// position-scoped. Serialized directly into the diagnostics Jinja template.
#[derive(Serialize)]
pub(super) struct DiagnosticsView<'a> {
    pub items: &'a [DiagnosticRow],
}

/// Unified result from `LspFeature::query()`.
///
/// Both markdown views (`render_view`) and symlink targets (`into_targets`)
/// consume this, eliminating the previous duplication between view render
/// methods and `query_targets`.
pub(super) enum QueryResult {
    Locations(Vec<Location>),
    HierarchyItems(Vec<HierarchyRow>),
    Hover(Option<Hover>),
    InlayHints(Vec<InlayHint>),
}

/// Methods for [`QueryResult`].
impl QueryResult {
    /// Render this result into template bytes via the appropriate view struct.
    ///
    /// Paths from LSP responses (overlay-rooted) are rewritten to FUSE paths
    /// for user-facing display.
    pub(super) fn render_view(self, engine: &TemplateEngine, template: &str, resolver: &PathResolver) -> Vec<u8> {
        match self {
            Self::Locations(locs) => engine.render_bytes(template, &LocationsView::from_locations(&locs, resolver)),
            Self::HierarchyItems(items) => {
                let fuse_items: Vec<HierarchyRow> = items
                    .into_iter()
                    .map(|mut row| {
                        row.file = resolver.rewrite_to_fuse(&row.file);
                        row
                    })
                    .collect();
                engine.render_bytes(template, &HierarchyListView { items: &fuse_items })
            }
            Self::Hover(hover) => engine.render_bytes(template, &HoverView::new(hover.as_ref())),
            Self::InlayHints(hints) => engine.render_bytes(template, &InlayHintsView::from_hints(&hints)),
        }
    }

    /// Extract raw targets for symlink directory population.
    ///
    /// Paths from LSP responses (overlay-rooted) are rewritten to FUSE paths
    /// so that symlink resolution can match against `fuse_root`.
    pub(super) fn into_targets(self, resolver: &PathResolver) -> Vec<Target> {
        match self {
            Self::Locations(locs) => locs
                .iter()
                .map(|loc| Target {
                    abs_path: resolver.rewrite_to_fuse(&uri_to_file_path(&loc.uri)),
                    line: loc.range.start.line,
                    name: None,
                })
                .collect(),
            Self::HierarchyItems(items) => items
                .into_iter()
                .map(|item| Target {
                    abs_path: resolver.rewrite_to_fuse(&item.file),
                    line: item.line.saturating_sub(1), // HierarchyRow stores 1-based
                    name: Some(item.name).filter(|n| !n.is_empty()),
                })
                .collect(),
            Self::Hover(_) | Self::InlayHints(_) => Vec::new(),
        }
    }
}
