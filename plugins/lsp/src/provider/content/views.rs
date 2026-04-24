//! View types for rendering LSP query results into Jinja templates.
//!
//! Each LSP feature has a corresponding view struct that transforms raw LSP
//! protocol responses into serializable data for template rendering.
//! [`QueryResult`] is the unified return type from all LSP queries.

use std::path::PathBuf;

use lsp_types::{CallHierarchyItem, Hover, HoverContents, InlayHint, InlayHintLabel, Location, MarkedString, Position};
use nyne::templates::TemplateEngine;
use serde::Serialize;

use super::feature::Target;
use crate::session::diagnostic_view::DiagnosticRow;
use crate::session::path::PathResolver;
use crate::session::uri::uri_to_file_path;

/// Extract display text from hover contents.
fn extract_hover_content(contents: &HoverContents) -> String {
    match contents {
        HoverContents::Scalar(value) => marked_string_to_text(value),
        HoverContents::Array(values) => values
            .iter()
            .map(marked_string_to_text)
            .collect::<Vec<_>>()
            .join("\n\n"),
        HoverContents::Markup(markup) => markup.value.clone(),
    }
}

/// Convert a `MarkedString` to plain text.
fn marked_string_to_text(ms: &MarkedString) -> String {
    match ms {
        MarkedString::String(s) => s.clone(),
        MarkedString::LanguageString(ls) => format!("```{}\n{}\n```", ls.language, ls.value),
    }
}

/// Extract a display string from an inlay hint label.
fn extract_inlay_label(label: &InlayHintLabel) -> String {
    match label {
        InlayHintLabel::String(s) => s.clone(),
        InlayHintLabel::LabelParts(parts) => parts.iter().map(|p| p.value.as_str()).collect(),
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
    ///
    /// Paths are relativized by stripping the source root — the same
    /// approach used by symlink target construction in [`into_targets`].
    pub(super) fn from_locations(locs: &[Location], resolver: &PathResolver) -> Self {
        let source_root = resolver.source_root();
        Self {
            locations: locs
                .iter()
                .map(|loc| {
                    let abs_path = uri_to_file_path(&loc.uri);
                    LocationRow {
                        file: abs_path
                            .strip_prefix(source_root)
                            .unwrap_or(&abs_path)
                            .display()
                            .to_string(),
                        line: loc.range.start.line + 1,
                        col: loc.range.start.character + 1,
                    }
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
pub struct HierarchyRow {
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

/// Unified result from `Feature::query()`.
///
/// Both markdown views (`render_view`) and symlink targets (`into_targets`)
/// consume this.
pub enum QueryResult {
    Locations(Vec<Location>),
    HierarchyItems(Vec<HierarchyRow>),
    Hover(Option<Hover>),
    InlayHints(Vec<InlayHint>),
}

/// Methods for [`QueryResult`].
impl QueryResult {
    /// Render this result into template bytes via the appropriate view struct.
    ///
    /// Paths from LSP responses are relativized by stripping the source root
    /// — the same approach used by symlink target construction in [`into_targets`].
    #[allow(clippy::excessive_nesting)]
    pub(crate) fn render_view(self, engine: &TemplateEngine, template: &str, resolver: &PathResolver) -> Vec<u8> {
        match self {
            Self::Locations(locs) => engine.render_bytes(template, &LocationsView::from_locations(&locs, resolver)),
            Self::HierarchyItems(items) => {
                let source_root = resolver.source_root();
                let rel_items: Vec<HierarchyRow> = items
                    .into_iter()
                    .map(|mut row| {
                        if let Ok(rel) = row.file.strip_prefix(source_root) {
                            row.file = rel.to_path_buf();
                        }
                        row
                    })
                    .collect();
                engine.render_bytes(template, &HierarchyListView { items: &rel_items })
            }
            Self::Hover(hover) => engine.render_bytes(template, &HoverView::new(hover.as_ref())),
            Self::InlayHints(hints) => engine.render_bytes(template, &InlayHintsView::from_hints(&hints)),
        }
    }

    /// Extract resolved targets for symlink directory population.
    ///
    /// Paths are relativized by stripping the source root — the same
    /// approach used by template views in [`render_view`]. Targets whose
    /// paths fall outside the source root (external files) are dropped.
    pub(crate) fn into_targets(self, resolver: &PathResolver) -> Vec<Target> {
        let source_root = resolver.source_root();
        match self {
            Self::Locations(locs) => locs
                .iter()
                .filter_map(|loc| {
                    let abs_path = uri_to_file_path(&loc.uri);
                    Some(Target {
                        rel_path: abs_path.strip_prefix(source_root).ok()?.to_path_buf(),
                        line: loc.range.start.line,
                        name: None,
                    })
                })
                .collect(),
            Self::HierarchyItems(items) => items
                .into_iter()
                .filter_map(|item| {
                    Some(Target {
                        rel_path: item.file.strip_prefix(source_root).ok()?.to_path_buf(),
                        line: item.line.saturating_sub(1), // HierarchyRow stores 1-based
                        name: Some(item.name).filter(|n| !n.is_empty()),
                    })
                })
                .collect(),
            Self::Hover(_) | Self::InlayHints(_) => Vec::new(),
        }
    }
}
