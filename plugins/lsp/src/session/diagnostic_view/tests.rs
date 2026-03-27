use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};

use super::{diagnostics_to_rows, severity_label};

/// Tests that all severity levels map to the expected labels.
#[test]
fn severity_labels() {
    assert_eq!(severity_label(Some(DiagnosticSeverity::ERROR)), "error");
    assert_eq!(severity_label(Some(DiagnosticSeverity::WARNING)), "warning");
    assert_eq!(severity_label(Some(DiagnosticSeverity::INFORMATION)), "info");
    assert_eq!(severity_label(Some(DiagnosticSeverity::HINT)), "hint");
    assert_eq!(severity_label(None), "unknown");
}

/// Build a minimal diagnostic at the given position for testing.
fn diag(line: u32, col: u32, severity: DiagnosticSeverity, message: &str) -> Diagnostic {
    Diagnostic {
        range: Range::new(Position::new(line, col), Position::new(line, col + 1)),
        severity: Some(severity),
        message: message.to_owned(),
        ..Default::default()
    }
}

/// Tests that row positions are 1-based (offset from 0-based LSP positions).
#[test]
fn rows_offset_by_one() {
    let diags = vec![diag(0, 0, DiagnosticSeverity::ERROR, "err")];
    let rows = diagnostics_to_rows(&diags);
    assert_eq!(rows.len(), 1);
    // LSP positions are 0-based; rows are 1-based for display.
    assert_eq!(rows[0].line, 1);
    assert_eq!(rows[0].col, 1);
    assert_eq!(rows[0].severity, "error");
    assert_eq!(rows[0].message, "err");
}

/// Tests that string diagnostic codes and source are extracted into rows.
#[test]
fn code_extraction() {
    let mut d = diag(5, 10, DiagnosticSeverity::WARNING, "warn");
    d.code = Some(NumberOrString::String("E0308".to_owned()));
    d.source = Some("rust-analyzer".to_owned());

    let rows = diagnostics_to_rows(&[d]);
    assert_eq!(rows[0].code, "E0308");
    assert_eq!(rows[0].source, "rust-analyzer");
}

/// Tests that numeric diagnostic codes are converted to strings.
#[test]
fn numeric_code() {
    let mut d = diag(0, 0, DiagnosticSeverity::HINT, "hint");
    d.code = Some(NumberOrString::Number(42));

    let rows = diagnostics_to_rows(&[d]);
    assert_eq!(rows[0].code, "42");
}

/// Tests that missing code and source default to empty strings.
#[test]
fn missing_optional_fields() {
    let d = diag(0, 0, DiagnosticSeverity::ERROR, "bare");
    let rows = diagnostics_to_rows(&[d]);
    assert_eq!(rows[0].code, "");
    assert_eq!(rows[0].source, "");
}

/// Tests that empty diagnostics input produces empty rows.
#[test]
fn empty_input() {
    assert!(diagnostics_to_rows(&[]).is_empty());
}
