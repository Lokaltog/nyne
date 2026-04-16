use rstest::rstest;

use super::*;

/// Parsing a `name:<spec>` suffix into a [`SliceSpec`] — accepts valid forms and rejects malformed ones.
#[rstest]
#[case::single("lines:5", Some(("lines", SliceSpec::Single(5))))]
#[case::range("LOG.md:5-10", Some(("LOG.md", SliceSpec::Range(5, 10))))]
#[case::tail("LOG.md:-10", Some(("LOG.md", SliceSpec::Tail(10))))]
#[case::no_colon("lines", None)]
#[case::empty_base(":5", None)]
#[case::zero_rejected("lines:0", None)]
#[case::inverted_range_rejected("lines:10-5", None)]
#[case::zero_tail_rejected("lines:-0", None)]
fn parse_suffix(#[case] input: &str, #[case] expected: Option<(&str, SliceSpec)>) {
    assert_eq!(parse_slice_suffix(input), expected);
}

/// [`SliceSpec::apply`] returns the selected elements, clamping out-of-range values.
#[rstest]
#[case::single(vec!["a", "b", "c", "d", "e"], SliceSpec::Single(3), vec!["c"])]
#[case::range(vec!["a", "b", "c", "d", "e"], SliceSpec::Range(2, 4), vec!["b", "c", "d"])]
#[case::tail(vec!["a", "b", "c", "d", "e"], SliceSpec::Tail(2), vec!["d", "e"])]
#[case::range_clamps_high(vec!["a", "b", "c"], SliceSpec::Range(1, 100), vec!["a", "b", "c"])]
#[case::single_out_of_range(vec!["a", "b", "c"], SliceSpec::Single(99), vec![])]
#[case::tail_clamps_high(vec!["a", "b", "c"], SliceSpec::Tail(100), vec!["a", "b", "c"])]
fn apply(#[case] items: Vec<&str>, #[case] spec: SliceSpec, #[case] expected: Vec<&str>) {
    assert_eq!(spec.apply(&items), expected.as_slice());
}

/// [`SliceSpec::index_range`] produces the right 0-based half-open range, clamping out-of-range values.
#[rstest]
#[case::single(SliceSpec::Single(3), 5, 2..3)]
#[case::range(SliceSpec::Range(2, 4), 5, 1..4)]
#[case::tail(SliceSpec::Tail(2), 5, 3..5)]
#[case::range_clamps_high(SliceSpec::Range(1, 100), 3, 0..3)]
#[case::single_out_of_range(SliceSpec::Single(99), 3, 3..3)]
#[case::tail_clamps_high(SliceSpec::Tail(100), 3, 0..3)]
fn index_range(#[case] spec: SliceSpec, #[case] len: usize, #[case] expected: std::ops::Range<usize>) {
    assert_eq!(spec.index_range(len), expected);
}
