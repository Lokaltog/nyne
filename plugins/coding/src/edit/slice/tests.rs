use super::*;

#[test]
fn parse_single() {
    assert_eq!(parse_slice_suffix("lines:5"), Some(("lines", SliceSpec::Single(5))));
}

#[test]
fn parse_range() {
    assert_eq!(
        parse_slice_suffix("LOG.md:5-10"),
        Some(("LOG.md", SliceSpec::Range(5, 10)))
    );
}

#[test]
fn parse_tail() {
    assert_eq!(parse_slice_suffix("LOG.md:-10"), Some(("LOG.md", SliceSpec::Tail(10))));
}

#[test]
fn parse_no_colon() {
    assert_eq!(parse_slice_suffix("lines"), None);
}

#[test]
fn parse_empty_base() {
    assert_eq!(parse_slice_suffix(":5"), None);
}

#[test]
fn parse_zero_rejected() {
    assert_eq!(parse_slice_suffix("lines:0"), None);
}

#[test]
fn parse_inverted_range_rejected() {
    assert_eq!(parse_slice_suffix("lines:10-5"), None);
}

#[test]
fn parse_zero_tail_rejected() {
    assert_eq!(parse_slice_suffix("lines:-0"), None);
}

#[test]
fn apply_single() {
    let items = vec!["a", "b", "c", "d", "e"];
    assert_eq!(SliceSpec::Single(3).apply(&items), &["c"]);
}

#[test]
fn apply_range() {
    let items = vec!["a", "b", "c", "d", "e"];
    assert_eq!(SliceSpec::Range(2, 4).apply(&items), &["b", "c", "d"]);
}

#[test]
fn apply_tail() {
    let items = vec!["a", "b", "c", "d", "e"];
    assert_eq!(SliceSpec::Tail(2).apply(&items), &["d", "e"]);
}

#[test]
fn apply_clamps_to_bounds() {
    let items = vec!["a", "b", "c"];
    assert_eq!(SliceSpec::Range(1, 100).apply(&items), &["a", "b", "c"]);
    assert_eq!(SliceSpec::Single(99).apply(&items), &[] as &[&str]);
    assert_eq!(SliceSpec::Tail(100).apply(&items), &["a", "b", "c"]);
}

#[test]
fn index_range_single() {
    assert_eq!(SliceSpec::Single(3).index_range(5), 2..3);
}

#[test]
fn index_range_range() {
    assert_eq!(SliceSpec::Range(2, 4).index_range(5), 1..4);
}

#[test]
fn index_range_tail() {
    assert_eq!(SliceSpec::Tail(2).index_range(5), 3..5);
}

#[test]
fn index_range_clamps_to_bounds() {
    assert_eq!(SliceSpec::Range(1, 100).index_range(3), 0..3);
    assert_eq!(SliceSpec::Single(99).index_range(3), 3..3);
    assert_eq!(SliceSpec::Tail(100).index_range(3), 0..3);
}
