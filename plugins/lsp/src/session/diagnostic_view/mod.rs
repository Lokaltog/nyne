//! Shared diagnostic formatting for template rendering.
//!
//! SSOT for the `lsp_types::Diagnostic` to [`DiagnosticRow`] conversion.
//! Used by both the `DIAGNOSTICS.md` provider and the post-tool-use hook,
//! ensuring consistent formatting across all diagnostic surfaces.

use lsp_types::{Diagnostic, DiagnosticSeverity};
use serde::Serialize;

/// A single diagnostic formatted for template rendering.
///
/// All fields are template-friendly: positions are 1-based (human-readable),
/// severity is a lowercase label string, and code/source are pre-extracted
/// from the LSP `Diagnostic`'s optional fields.
#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticRow {
    pub line: u32,
    pub col: u32,
    pub severity: &'static str,
    pub code: String,
    pub source: String,
    pub message: String,
}

/// Convert LSP diagnostics to template-ready rows.
///
/// Converts 0-based LSP positions to 1-based line/column numbers and
/// extracts optional fields (code, source) into plain strings. The
/// resulting rows are directly consumable by `minijinja` templates.
pub fn diagnostics_to_rows(diags: &[Diagnostic]) -> Vec<DiagnosticRow> {
    diags
        .iter()
        .map(|d| DiagnosticRow {
            line: d.range.start.line + 1,
            col: d.range.start.character + 1,
            severity: severity_label(d.severity),
            code: d
                .code
                .as_ref()
                .map(|c| match c {
                    lsp_types::NumberOrString::Number(n) => n.to_string(),
                    lsp_types::NumberOrString::String(s) => s.clone(),
                })
                .unwrap_or_default(),
            source: d.source.clone().unwrap_or_default(),
            message: d.message.clone(),
        })
        .collect()
}

/// Human-readable severity label.
pub const fn severity_label(s: Option<DiagnosticSeverity>) -> &'static str {
    match s {
        Some(DiagnosticSeverity::ERROR) => "error",
        Some(DiagnosticSeverity::WARNING) => "warning",
        Some(DiagnosticSeverity::INFORMATION) => "info",
        Some(DiagnosticSeverity::HINT) => "hint",
        _ => "unknown",
    }
}

/// Unit tests.
#[cfg(test)]
mod tests;
