use rstest::rstest;

use super::*;

/// Build a populated test table with `rows` rows of a single "COL" column.
fn populated(rows: usize) -> Table {
    let mut table = new_table();
    table.set_header(vec!["COL"]);
    for i in 0..rows {
        table.add_row(vec![format!("row-{i}")]);
    }
    table
}

#[test]
fn render_or_empty_returns_fallback_when_table_has_no_rows() {
    let out = render_or_empty(&new_table(), "No sessions.");
    assert!(out.contains("No sessions."), "empty-state message expected in: {out:?}");
    assert!(!out.contains("COL"), "header must not appear when empty: {out:?}");
}

#[rstest]
#[case::one_row(1)]
#[case::many_rows(5)]
fn render_or_empty_returns_header_and_rows_when_populated(#[case] rows: usize) {
    let out = render_or_empty(&populated(rows), "No sessions.");
    assert!(out.contains("COL"), "header expected in populated output: {out:?}");
    for i in 0..rows {
        assert!(out.contains(&format!("row-{i}")), "row {i} missing from: {out:?}");
    }
}

#[rstest]
#[case::one(["A"].as_slice())]
#[case::three(["A", "B", "C"].as_slice())]
fn bold_headers_preserves_label_text(#[case] labels: &[&str]) {
    // Table must render each label verbatim; the Bold attribute only
    // affects ANSI output, not the visible text.
    let mut table = new_table();
    match labels.len() {
        1 => table.set_header(bold_headers(["A"])),
        3 => table.set_header(bold_headers(["A", "B", "C"])),
        _ => unreachable!(),
    };
    table.add_row(vec!["x"; labels.len()]);
    let rendered = table.to_string();
    for label in labels {
        assert!(rendered.contains(label), "label {label:?} missing from: {rendered:?}");
    }
}
