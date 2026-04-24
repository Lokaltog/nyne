use std::ops::Range;

use rstest::rstest;

use super::*;
use crate::syntax::fragment::{Fragment, FragmentKind, FragmentMetadata, FragmentSpan, SymbolKind};

/// Helper: build a minimal code fragment with the given byte ranges for testing remapping.
fn code_fragment(byte_range: Range<usize>, name_byte_offset: usize, children: Vec<Fragment>) -> Fragment {
    Fragment {
        name: "test".to_owned(),
        kind: FragmentKind::Symbol(SymbolKind::Function),
        span: FragmentSpan::with_children(byte_range, name_byte_offset, &children),
        signature: None,
        visibility: None,
        metadata: None,
        children,
        parent_name: None,
        fs_name: None,
    }
}

/// Verifies `SpanMap::build` across content/len/clamping/empty-input variants.
/// `to_real_checks` asserts `map.to_real(virt) == real` for each `(virt, real)` tuple.
#[rstest]
#[case::normal_content_and_map(
    "AAAbbbCCCdddEEE",
    &[3..6, 9..12],
    "bbbddd",
    &[(0, 3), (3, 9)],
)]
#[case::skip_zero_length_regions("hello world", &[0..0, 6..11], "world", &[])]
#[case::empty_regions_empty_content("anything", &[], "", &[])]
#[case::clamp_out_of_bounds("short", &[3..103], "rt", &[])]
fn build_cases(
    #[case] source: &str,
    #[case] regions: &[Range<usize>],
    #[case] expected_content: &str,
    #[case] to_real_checks: &[(usize, usize)],
) {
    let (map, content) = SpanMap::build(source, regions);
    assert_eq!(content, expected_content);
    assert_eq!(
        map.virtual_len(),
        expected_content.len(),
        "virtual_len matches content len"
    );
    for (virt, real) in to_real_checks {
        assert_eq!(map.to_real(*virt), *real);
    }
}

/// Verifies `SpanMap::new` across region-count/gap variations, including the
/// `to_real(virtual_len)` edge case that maps to one past the last real byte.
#[rstest]
#[case::empty_map(&[], 0, &[])]
#[case::single_contiguous_identity(
    &[(0, 10)],
    10,
    &[(0, 0), (5, 5), (9, 9)],
)]
#[case::two_disjoint(
    &[(10, 10), (30, 10)],
    20,
    &[(0, 10), (5, 15), (9, 19), (10, 30), (15, 35), (19, 39)],
)]
#[case::three_regions_varying_gaps(
    &[(5, 3), (20, 5), (50, 2)],
    10,
    &[(0, 5), (3, 20), (8, 50), (2, 7), (7, 24), (9, 51)],
)]
#[case::skip_zero_length_region(
    &[(10, 0), (20, 5)],
    5,
    &[(0, 20), (4, 24)],
)]
#[case::to_real_at_virtual_len_maps_past_last(
    &[(10, 5), (30, 5)],
    10,
    &[(10, 35)],
)]
fn span_map_new_cases(
    #[case] regions: &[(usize, usize)],
    #[case] expected_virtual_len: usize,
    #[case] to_real_checks: &[(usize, usize)],
) {
    let map = SpanMap::new(regions);
    assert_eq!(map.virtual_len(), expected_virtual_len);
    for (virt, real) in to_real_checks {
        assert_eq!(map.to_real(*virt), *real, "to_real({virt})");
    }
}

/// Verifies `remap_range` across single-region, boundary-exact, cross-boundary
/// (clamping), and empty-range (identity-at-start) scenarios.
#[rstest]
#[case::within_single_region(&[(10, 20)], 5..15, 15..25)]
#[case::exact_first_region(&[(10, 10), (30, 10)], 0..10, 10..20)]
#[case::exact_second_region(&[(10, 10), (30, 10)], 10..20, 30..40)]
#[case::spans_boundary_clamps_to_region_end(&[(10, 10), (30, 10)], 5..15, 15..20)]
#[case::empty_range_identity_at_start(&[(100, 10)], 5..5, 105..105)]
fn remap_range_cases(
    #[case] regions: &[(usize, usize)],
    #[case] virtual_range: Range<usize>,
    #[case] expected_real: Range<usize>,
) {
    assert_eq!(SpanMap::new(regions).remap_range(virtual_range), expected_real);
}

/// Verifies that `remap_fragment` remaps `byte_range` and `name_byte_offset` correctly.
#[rstest]
fn remap_fragment_basic() {
    let map = SpanMap::new(&[(100, 50)]);

    let frag = code_fragment(5..20, 5, vec![]);

    let remapped = map.remap_fragment(frag);
    assert_eq!(remapped.span.byte_range, 105..120);
    assert_eq!(remapped.span.full_span, 105..120);
    assert_eq!(remapped.span.name_byte_offset, 105);
}

/// Verifies that `remap_fragment` recursively remaps nested children.
#[rstest]
fn remap_fragment_recursive_children() {
    let map = SpanMap::new(&[(200, 100)]);

    let child = code_fragment(20..40, 20, vec![]);
    let parent = code_fragment(0..80, 0, vec![child]);

    let remapped = map.remap_fragment(parent);
    assert_eq!(remapped.span.byte_range, 200..280);
    assert_eq!(remapped.span.name_byte_offset, 200);

    assert_eq!(remapped.children.len(), 1);
    let child = &remapped.children[0];
    assert_eq!(child.span.byte_range, 220..240);
    assert_eq!(child.span.full_span, 220..240);
    assert_eq!(child.span.name_byte_offset, 220);
}

/// Verifies that `remap_fragment` preserves name, signature, `parent_name`, and `fs_name`.
#[rstest]
fn remap_fragment_preserves_non_byte_fields() {
    let map = SpanMap::new(&[(50, 30)]);

    let mut frag = code_fragment(0..10, 0, vec![]);
    frag.name = "my_func".to_owned();
    frag.signature = Some("fn my_func()".to_owned());
    frag.parent_name = Some("parent".to_owned());
    frag.fs_name = Some("my_func".to_owned());

    let remapped = map.remap_fragment(frag);
    assert_eq!(remapped.name, "my_func");
    assert_eq!(remapped.signature.as_deref(), Some("fn my_func()"));
    assert_eq!(remapped.parent_name.as_deref(), Some("parent"));
    assert_eq!(remapped.fs_name.as_deref(), Some("my_func"));
}

/// Verifies `remap_fragment` preserves the fragment's `kind` and `metadata`
/// unchanged (byte ranges still remap as expected). Covers both Section+Document
/// and CodeBlock+CodeBlock pairings.
#[rstest]
#[case::section_document_metadata(
    FragmentKind::Section { level: 2 },
    FragmentMetadata::Document { index: 3 },
)]
#[case::code_block_metadata(
    FragmentKind::CodeBlock { lang: Some("rust".to_owned()) },
    FragmentMetadata::CodeBlock { index: 1 },
)]
fn remap_fragment_preserves_kind_and_metadata(#[case] kind: FragmentKind, #[case] metadata: FragmentMetadata) {
    let map = SpanMap::new(&[(100, 50)]);
    let frag = Fragment {
        name: "test".to_owned(),
        kind: kind.clone(),
        span: FragmentSpan::leaf(0..20, 0),
        signature: None,
        visibility: None,
        metadata: Some(metadata.clone()),
        children: vec![],
        parent_name: None,
        fs_name: None,
    };

    let remapped = map.remap_fragment(frag);
    assert_eq!(remapped.span.byte_range, 100..120);
    assert_eq!(remapped.span.full_span, 100..120);
    assert_eq!(remapped.metadata, Some(metadata));
    assert_eq!(remapped.kind, kind);
}

/// Verifies that remapped byte ranges extract the correct text from the original source.
#[rstest]
fn round_trip_extract_via_remapped_ranges() {
    // Simulates the real use case: compound source with template gaps,
    // inner content extracted and remapped, then byte ranges verified
    // against the original source.
    let source = "{% block title %}The Title{% endblock %}\n# Heading\nBody text\n";
    //            |---- template ----|--- content ---|--- template ---|content...|

    // Content regions (manually identified):
    let regions = &[
        17..26, // "The Title" at offset 17
        40..62, // "\n# Heading\nBody text\n" at offset 40
    ];

    let (map, content) = SpanMap::build(source, regions);
    assert_eq!(content, "The Title\n# Heading\nBody text\n");

    // Suppose the inner decomposer found "Heading" at virtual [12..19)
    let virtual_range = 12..19;
    let real_range = map.remap_range(virtual_range);

    // Verify we can extract the same text from the original source
    assert_eq!(
        source.get(real_range.clone()),
        Some("Heading"),
        "remapped range {real_range:?} should extract 'Heading' from original source"
    );
}
