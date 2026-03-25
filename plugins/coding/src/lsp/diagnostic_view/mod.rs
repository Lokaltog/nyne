// Shared diagnostic formatting for template rendering.
//
// SSOT for the `lsp_types::Diagnostic` → `DiagnosticRow` conversion.
// Used by both the DIAGNOSTICS.md provider and the post-tool-use hook.

use lsp_types::{Diagnostic, DiagnosticSeverity};
use serde::Serialize;

/// A diagnostic row for template rendering.
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
