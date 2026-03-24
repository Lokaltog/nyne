use super::*;

/// Tests that a `name:N` suffix parses into a Single spec.
#[test]
fn parse_single() {
    assert_eq!(parse_slice_suffix("lines:5"), Some(("lines", SliceSpec::Single(5))));
}

/// Tests that a `name:M-N` suffix parses into a Range spec.
#[test]
fn parse_range() {
    assert_eq!(
        parse_slice_suffix("LOG.md:5-10"),
        Some(("LOG.md", SliceSpec::Range(5, 10)))
    );
}

/// Tests that a `name:-N` suffix parses into a Tail spec.
#[test]
fn parse_tail() {
    assert_eq!(parse_slice_suffix("LOG.md:-10"), Some(("LOG.md", SliceSpec::Tail(10))));
}

/// Verifies that a name without a colon returns None.
#[test]
fn parse_no_colon() {
    assert_eq!(parse_slice_suffix("lines"), None);
}

/// Verifies that a colon with no base name is rejected.
#[test]
fn parse_empty_base() {
    assert_eq!(parse_slice_suffix(":5"), None);
}

/// Verifies that a zero index is rejected (1-based indexing).
#[test]
fn parse_zero_rejected() {
    assert_eq!(parse_slice_suffix("lines:0"), None);
}

/// Verifies that an inverted range (start > end) is rejected.
#[test]
fn parse_inverted_range_rejected() {
    assert_eq!(parse_slice_suffix("lines:10-5"), None);
}

/// Verifies that a zero tail count is rejected.
#[test]
fn parse_zero_tail_rejected() {
    assert_eq!(parse_slice_suffix("lines:-0"), None);
}

/// Tests that apply with a single index returns exactly one element.
#[test]
fn apply_single() {
    let items = vec!["a", "b", "c", "d", "e"];
    assert_eq!(SliceSpec::Single(3).apply(&items), &["c"]);
}

/// Tests that apply with a range spec returns the correct sub-slice.
#[test]
fn apply_range() {
    let items = vec!["a", "b", "c", "d", "e"];
    assert_eq!(SliceSpec::Range(2, 4).apply(&items), &["b", "c", "d"]);
}

/// Tests that apply with a tail spec returns the last N elements.
#[test]
fn apply_tail() {
    let items = vec!["a", "b", "c", "d", "e"];
    assert_eq!(SliceSpec::Tail(2).apply(&items), &["d", "e"]);
}

/// Verifies that apply clamps out-of-range indices to collection bounds.
#[test]
fn apply_clamps_to_bounds() {
    let items = vec!["a", "b", "c"];
    assert_eq!(SliceSpec::Range(1, 100).apply(&items), &["a", "b", "c"]);
    assert_eq!(SliceSpec::Single(99).apply(&items), &[] as &[&str]);
    assert_eq!(SliceSpec::Tail(100).apply(&items), &["a", "b", "c"]);
}

/// Tests that index_range for a single index produces a one-element range.
#[test]
fn index_range_single() {
    assert_eq!(SliceSpec::Single(3).index_range(5), 2..3);
}

/// Tests that index_range converts a 1-based range to a 0-based half-open range.
#[test]
fn index_range_range() {
    assert_eq!(SliceSpec::Range(2, 4).index_range(5), 1..4);
}

/// Tests that index_range for a tail spec covers the last N indices.
#[test]
fn index_range_tail() {
    assert_eq!(SliceSpec::Tail(2).index_range(5), 3..5);
}

/// Verifies that index_range clamps out-of-range values to collection bounds.
#[test]
fn index_range_clamps_to_bounds() {
    assert_eq!(SliceSpec::Range(1, 100).index_range(3), 0..3);
    assert_eq!(SliceSpec::Single(99).index_range(3), 3..3);
    assert_eq!(SliceSpec::Tail(100).index_range(3), 0..3);
}
