// View structs and TemplateView impls for LSP result rendering.

use std::path::PathBuf;
use std::sync::Arc;

use color_eyre::eyre::{Result, eyre};
use lsp_types::{CallHierarchyItem, Hover, InlayHint, Location, Position, TypeHierarchyItem};
use nyne::templates::{TemplateEngine, TemplateView};
use serde::Serialize;

use super::feature::{LspFeature, LspTarget};
use super::format::{extract_hover_content, extract_inlay_label, lsp_symbol_kind_label};
use crate::lsp::diagnostic_view::{DiagnosticRow, diagnostics_to_rows};
use crate::lsp::handle::{LspHandle, SymbolQuery};
use crate::lsp::path::LspPathResolver;
use crate::lsp::uri::uri_to_file_path;
use crate::providers::fragment_resolver::FragmentResolver;

/// Per-symbol LSP view — acquires a `FileQuery` at read time and
/// delegates to `LspFeature::query()` + `LspQueryResult::render_view()`.
///
/// Replaces the previous 5 separate `TemplateView` impls with one.
pub(super) struct SymbolLspView {
    pub query: SymbolQuery,
    pub feature: LspFeature,
    pub resolver: FragmentResolver,
    pub fragment_path: Vec<String>,
}

impl TemplateView for SymbolLspView {
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
pub(super) struct DiagnosticsLspView(pub Arc<LspHandle>);

impl TemplateView for DiagnosticsLspView {
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

impl LocationsView {
    pub fn from_locations(locs: &[Location], resolver: &LspPathResolver) -> Self {
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

impl HoverView {
    pub fn new(hover: Option<&Hover>) -> Self {
        Self {
            content: hover.map(|h| extract_hover_content(&h.contents)).unwrap_or_default(),
        }
    }
}

/// Unified view for hierarchy results (callers, deps, supertypes, subtypes).
///
/// Callers/deps templates iterate `items` with `name`/`file`/`line`.
/// Supertypes/subtypes templates additionally use `kind`.
#[derive(Serialize)]
pub(super) struct HierarchyListView<'a> {
    pub items: &'a [HierarchyRow],
}

/// Row shared by all hierarchy results — call hierarchy and type hierarchy.
///
/// `kind` is empty for call hierarchy items (templates ignore it).
#[derive(Clone, Serialize)]
pub(super) struct HierarchyRow {
    pub name: String,
    pub kind: &'static str,
    pub file: PathBuf,
    pub line: u32,
}

/// Extract a `HierarchyRow` from a `CallHierarchyItem`.
pub(super) fn hierarchy_item(item: &CallHierarchyItem) -> HierarchyRow {
    HierarchyRow {
        name: item.name.clone(),
        kind: "",
        file: uri_to_file_path(&item.uri),
        line: item.selection_range.start.line + 1,
    }
}

/// Extract a `HierarchyRow` from a `TypeHierarchyItem`.
pub(super) fn type_hierarchy_item(item: &TypeHierarchyItem) -> HierarchyRow {
    HierarchyRow {
        name: item.name.clone(),
        kind: lsp_symbol_kind_label(item.kind),
        file: uri_to_file_path(&item.uri),
        line: item.selection_range.start.line + 1,
    }
}

/// View for inlay hints rendering.
#[derive(Serialize)]
pub(super) struct InlayHintsRenderView {
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

impl InlayHintsRenderView {
    pub fn from_hints(raw: &[InlayHint]) -> Self {
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

/// View for rendering diagnostic results.
#[derive(Serialize)]
pub(super) struct DiagnosticsView<'a> {
    pub items: &'a [DiagnosticRow],
}

/// Unified result from `LspFeature::query()`.
///
/// Both markdown views (`render_view`) and symlink targets (`into_targets`)
/// consume this, eliminating the previous duplication between view render
/// methods and `query_targets`.
pub(super) enum LspQueryResult {
    Locations(Vec<Location>),
    HierarchyItems(Vec<HierarchyRow>),
    Hover(Option<Hover>),
    InlayHints(Vec<InlayHint>),
}

impl LspQueryResult {
    /// Render this result into template bytes via the appropriate view struct.
    ///
    /// Paths from LSP responses (overlay-rooted) are rewritten to FUSE paths
    /// for user-facing display.
    pub fn render_view(self, engine: &TemplateEngine, template: &str, resolver: &LspPathResolver) -> Vec<u8> {
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
            Self::InlayHints(hints) => engine.render_bytes(template, &InlayHintsRenderView::from_hints(&hints)),
        }
    }

    /// Extract raw targets for symlink directory population.
    ///
    /// Paths from LSP responses (overlay-rooted) are rewritten to FUSE paths
    /// so that symlink resolution can match against `fuse_root`.
    pub fn into_targets(self, resolver: &LspPathResolver) -> Vec<LspTarget> {
        match self {
            Self::Locations(locs) => locs
                .iter()
                .map(|loc| LspTarget {
                    abs_path: resolver.rewrite_to_fuse(&uri_to_file_path(&loc.uri)),
                    line: loc.range.start.line,
                    name: None,
                })
                .collect(),
            Self::HierarchyItems(items) => items
                .into_iter()
                .map(|item| LspTarget {
                    abs_path: resolver.rewrite_to_fuse(&item.file),
                    line: item.line.saturating_sub(1), // HierarchyRow stores 1-based
                    name: Some(item.name).filter(|n| !n.is_empty()),
                })
                .collect(),
            Self::Hover(_) | Self::InlayHints(_) => Vec::new(),
        }
    }
}
