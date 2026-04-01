use std::ops::Range;

use super::*;
use crate::syntax::fragment::{Fragment, FragmentKind, FragmentMetadata, SymbolKind};

/// Helper: build a minimal code fragment with the given byte ranges for testing remapping.
fn code_fragment(byte_range: Range<usize>, name_byte_offset: usize, children: Vec<Fragment>) -> Fragment {
    Fragment::new(
        "test".to_owned(),
        FragmentKind::Symbol(SymbolKind::Function),
        byte_range,
        None,
        None,
        None,
        name_byte_offset,
        children,
        None,
    )
}

/// Verifies that `SpanMap::build` produces correct virtual content and offset mapping.
#[test]
fn build_produces_correct_content_and_map() {
    let source = "AAAbbbCCCdddEEE";
    //            0123456789...
    // Regions: [3..6) = "bbb", [9..12) = "ddd"
    let (map, content) = SpanMap::build(source, &[3..6, 9..12]);

    assert_eq!(content, "bbbddd");
    assert_eq!(map.virtual_len(), 6);
    assert_eq!(map.to_real(0), 3); // 'b' at real offset 3
    assert_eq!(map.to_real(3), 9); // 'd' at real offset 9
}

/// Verifies that zero-length regions are skipped during `SpanMap` construction.
#[test]
fn build_skips_zero_length_regions() {
    let source = "hello world";
    let (map, content) = SpanMap::build(source, &[0..0, 6..11]);

    assert_eq!(content, "world");
    assert_eq!(map.virtual_len(), 5);
}

/// Verifies that empty region list produces empty content and zero-length map.
#[test]
fn build_empty_regions_produces_empty_content() {
    let (map, content) = SpanMap::build("anything", &[]);

    assert_eq!(content, "");
    assert_eq!(map.virtual_len(), 0);
}

/// Verifies that `SpanMap::build` clamps regions that extend past source bounds.
#[test]
fn build_clamps_to_source_bounds() {
    let source = "short";
    // Region extends past source end — should not panic, just clamp.
    let (map, content) = SpanMap::build(source, &[3..103]);

    assert_eq!(content, "rt");
    // Critical invariant: virtual_len must match actual content length.
    // A previous bug built the map from unclamped regions (virtual_len=100)
    // while content used clamped regions (len=2).
    assert_eq!(map.virtual_len(), content.len());
    assert_eq!(map.virtual_len(), 2);
}

/// Verifies that a single contiguous region starting at zero is an identity mapping.
#[test]
fn single_contiguous_region_identity() {
    // Region starting at real offset 0 — identity mapping.
    let map = SpanMap::new(&[(0, 10)]);
    assert_eq!(map.virtual_len(), 10);
    assert_eq!(map.to_real(0), 0);
    assert_eq!(map.to_real(5), 5);
    assert_eq!(map.to_real(9), 9);
}

/// Verifies that two disjoint regions map virtual offsets to correct real offsets.
#[test]
fn two_disjoint_regions() {
    // Region 1: real [10..20), Region 2: real [30..40)
    // Virtual [0..10) → real [10..20), virtual [10..20) → real [30..40)
    let map = SpanMap::new(&[(10, 10), (30, 10)]);
    assert_eq!(map.virtual_len(), 20);

    // First region
    assert_eq!(map.to_real(0), 10);
    assert_eq!(map.to_real(5), 15);
    assert_eq!(map.to_real(9), 19);

    // Second region
    assert_eq!(map.to_real(10), 30);
    assert_eq!(map.to_real(15), 35);
    assert_eq!(map.to_real(19), 39);
}

/// Verifies that three regions with varying gaps map every boundary correctly.
#[test]
fn three_regions_every_boundary() {
    // Three regions with varying gaps:
    // real [5..8)   len=3  → virtual [0..3)
    // real [20..25) len=5  → virtual [3..8)
    // real [50..52) len=2  → virtual [8..10)
    let map = SpanMap::new(&[(5, 3), (20, 5), (50, 2)]);
    assert_eq!(map.virtual_len(), 10);

    // Region boundaries — start of each
    assert_eq!(map.to_real(0), 5);
    assert_eq!(map.to_real(3), 20);
    assert_eq!(map.to_real(8), 50);

    // Last inclusive byte of each
    assert_eq!(map.to_real(2), 7);
    assert_eq!(map.to_real(7), 24);
    assert_eq!(map.to_real(9), 51);
}

/// Verifies that `to_real` at `virtual_len` maps to one past the last real byte.
#[test]
fn to_real_at_virtual_len() {
    // virtual_len is a valid exclusive-end offset. It should map via
    // the last region (one past its last byte), not fall through.
    let map = SpanMap::new(&[(10, 5), (30, 5)]);
    assert_eq!(map.virtual_len(), 10);

    // virtual 10 → last region starts at virtual 5, real 30.
    // offset_within = 10 - 5 = 5 → real 35 (one past [30..35))
    assert_eq!(map.to_real(10), 35);
}

/// Verifies that `remap_range` maps a virtual range within a single region correctly.
#[test]
fn remap_range_within_single_region() {
    let map = SpanMap::new(&[(10, 20)]);
    assert_eq!(map.remap_range(5..15), 15..25);
}

/// Verifies that `remap_range` handles ranges at region boundaries and cross-boundary clamping.
#[test]
fn remap_range_at_region_boundaries() {
    // Two regions: real [10..20), real [30..40)
    let map = SpanMap::new(&[(10, 10), (30, 10)]);

    // Range exactly covering first region — half-open end stays in region 1
    assert_eq!(map.remap_range(0..10), 10..20);

    // Range exactly covering second region
    assert_eq!(map.remap_range(10..20), 30..40);

    // Range spanning boundary — clamped to start region's real end
    assert_eq!(map.remap_range(5..15), 15..20);
}

/// Verifies that an empty virtual range remaps to the corresponding real position.
#[test]
fn remap_range_empty_is_identity_at_start() {
    let map = SpanMap::new(&[(100, 10)]);
    // Empty range: start == end → maps start, end = start
    assert_eq!(map.remap_range(5..5), 105..105);
}

/// Verifies that `remap_fragment` remaps `byte_range` and `name_byte_offset` correctly.
#[test]
fn remap_fragment_basic() {
    let map = SpanMap::new(&[(100, 50)]);

    let frag = code_fragment(5..20, 5, vec![]);

    let remapped = map.remap_fragment(frag);
    assert_eq!(remapped.byte_range, 105..120);
    assert_eq!(remapped.full_span(), 105..120);
    assert_eq!(remapped.name_byte_offset, 105);
}

/// Verifies that `remap_fragment` remaps a parent fragment with children.
#[test]
fn remap_fragment_with_children() {
    let map = SpanMap::new(&[(100, 50)]);

    let frag = code_fragment(10..30, 10, vec![]);

    let remapped = map.remap_fragment(frag);
    assert_eq!(remapped.byte_range, 110..130);
    assert_eq!(remapped.full_span(), 110..130);
    assert_eq!(remapped.name_byte_offset, 110);
}

/// Verifies that `remap_fragment` recursively remaps nested children.
#[test]
fn remap_fragment_recursive_children() {
    let map = SpanMap::new(&[(200, 100)]);

    let child = code_fragment(20..40, 20, vec![]);
    let parent = code_fragment(0..80, 0, vec![child]);

    let remapped = map.remap_fragment(parent);
    assert_eq!(remapped.byte_range, 200..280);
    assert_eq!(remapped.name_byte_offset, 200);

    assert_eq!(remapped.children.len(), 1);
    let child = &remapped.children[0];
    assert_eq!(child.byte_range, 220..240);
    assert_eq!(child.full_span(), 220..240);
    assert_eq!(child.name_byte_offset, 220);
}

/// Verifies that `remap_fragment` preserves name, signature, `parent_name`, and `fs_name`.
#[test]
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

/// Verifies that `remap_fragment` preserves Section metadata (Document index) unchanged.
#[test]
fn remap_fragment_section_metadata_unchanged() {
    let map = SpanMap::new(&[(100, 50)]);

    let frag = Fragment::new(
        "section".to_owned(),
        FragmentKind::Section { level: 2 },
        0..20,
        None,
        None,
        Some(FragmentMetadata::Document { index: 3 }),
        0,
        vec![],
        None,
    );

    let remapped = map.remap_fragment(frag);
    assert_eq!(remapped.byte_range, 100..120);
    assert_eq!(remapped.full_span(), 100..120);
    assert_eq!(remapped.metadata, Some(FragmentMetadata::Document { index: 3 }));
}

/// Verifies that `remap_fragment` preserves `CodeBlock` metadata (index and lang) unchanged.
#[test]
fn remap_fragment_code_block_metadata_unchanged() {
    let map = SpanMap::new(&[(50, 30)]);

    let frag = Fragment::new(
        "code_block".to_owned(),
        FragmentKind::CodeBlock {
            lang: Some("rust".to_owned()),
        },
        5..15,
        None,
        None,
        Some(FragmentMetadata::CodeBlock { index: 1 }),
        5,
        vec![],
        None,
    );

    let remapped = map.remap_fragment(frag);
    assert_eq!(remapped.byte_range, 55..65);
    assert_eq!(remapped.metadata, Some(FragmentMetadata::CodeBlock { index: 1 }));
    assert_eq!(remapped.kind, FragmentKind::CodeBlock {
        lang: Some("rust".to_owned())
    });
}

/// Verifies that an empty `SpanMap` has zero virtual length.
#[test]
fn empty_map() {
    let map = SpanMap::new(&[]);
    assert_eq!(map.virtual_len(), 0);
}

/// Verifies that zero-length regions are skipped when constructing a `SpanMap`.
#[test]
fn zero_length_regions_skipped() {
    let map = SpanMap::new(&[(10, 0), (20, 5)]);
    assert_eq!(map.virtual_len(), 5);
    assert_eq!(map.to_real(0), 20);
    assert_eq!(map.to_real(4), 24);
}

/// Verifies that remapped byte ranges extract the correct text from the original source.
#[test]
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
