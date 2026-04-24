use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};
use rstest::rstest;

use super::{diagnostics_to_rows, severity_label};

/// Tests that every severity level (including unknown) maps to its display label.
#[rstest]
#[case::error(Some(DiagnosticSeverity::ERROR), "error")]
#[case::warning(Some(DiagnosticSeverity::WARNING), "warning")]
#[case::info(Some(DiagnosticSeverity::INFORMATION), "info")]
#[case::hint(Some(DiagnosticSeverity::HINT), "hint")]
#[case::unknown(None, "unknown")]
fn severity_labels(#[case] severity: Option<DiagnosticSeverity>, #[case] expected: &str) {
    assert_eq!(severity_label(severity), expected);
}

/// Build a diagnostic at `(line, col)` with optional `code` and `source` for testing.
fn diag(
    line: u32,
    col: u32,
    severity: DiagnosticSeverity,
    message: &str,
    code: Option<NumberOrString>,
    source: Option<&str>,
) -> Diagnostic {
    Diagnostic {
        range: Range::new(Position::new(line, col), Position::new(line, col + 1)),
        severity: Some(severity),
        message: message.to_owned(),
        code,
        source: source.map(str::to_owned),
        ..Default::default()
    }
}

/// Tests that `diagnostics_to_rows` maps LSP diagnostics to display rows correctly:
/// 1-based positions, severity labels, code coercion (string/numeric), and empty defaults.
///
/// Each expected-row tuple is `(line, col, severity, message, code, source)`.
#[rstest]
#[case::offset_by_one(
    vec![diag(0, 0, DiagnosticSeverity::ERROR, "err", None, None)],
    &[(1, 1, "error", "err", "", "")],
)]
#[case::code_string(
    vec![diag(5, 10, DiagnosticSeverity::WARNING, "warn",
        Some(NumberOrString::String("E0308".to_owned())), Some("rust-analyzer"))],
    &[(6, 11, "warning", "warn", "E0308", "rust-analyzer")],
)]
#[case::code_numeric(
    vec![diag(0, 0, DiagnosticSeverity::HINT, "hint",
        Some(NumberOrString::Number(42)), None)],
    &[(1, 1, "hint", "hint", "42", "")],
)]
#[case::missing_optional_fields(
    vec![diag(0, 0, DiagnosticSeverity::ERROR, "bare", None, None)],
    &[(1, 1, "error", "bare", "", "")],
)]
#[case::empty_input(vec![], &[])]
fn diagnostics_to_rows_cases(#[case] diags: Vec<Diagnostic>, #[case] expected: &[(u32, u32, &str, &str, &str, &str)]) {
    let rows = diagnostics_to_rows(&diags);
    assert_eq!(rows.len(), expected.len());
    for (row, (line, col, sev, msg, code, source)) in rows.iter().zip(expected) {
        assert_eq!(row.line, *line);
        assert_eq!(row.col, *col);
        assert_eq!(row.severity, *sev);
        assert_eq!(row.message, *msg);
        assert_eq!(row.code, *code);
        assert_eq!(row.source, *source);
    }
}
