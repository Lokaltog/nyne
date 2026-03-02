use std::ops::Range;

use super::*;
use crate::syntax::fragment::{Fragment, FragmentKind, FragmentMetadata, SymbolKind};

/// Helper: build a minimal code fragment with the given byte ranges for testing remapping.
fn code_fragment(
    byte_range: Range<usize>,
    full_span: Range<usize>,
    name_byte_offset: usize,
    doc_comment_range: Option<Range<usize>>,
    decorator_range: Option<Range<usize>>,
    children: Vec<Fragment>,
) -> Fragment {
    Fragment {
        name: "test".to_owned(),
        kind: FragmentKind::Symbol(SymbolKind::Function),
        byte_range,
        full_span,
        line_range: 0..1, // line_range is NOT remapped by SpanMap — caller recomputes
        signature: None,
        metadata: FragmentMetadata::Code {
            visibility: None,
            doc_comment_range,
            decorator_range,
        },
        name_byte_offset,
        children,
        parent_name: None,
        fs_name: None,
    }
}

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

#[test]
fn build_skips_zero_length_regions() {
    let source = "hello world";
    let (map, content) = SpanMap::build(source, &[0..0, 6..11]);

    assert_eq!(content, "world");
    assert_eq!(map.virtual_len(), 5);
}

#[test]
fn build_empty_regions_produces_empty_content() {
    let (map, content) = SpanMap::build("anything", &[]);

    assert_eq!(content, "");
    assert_eq!(map.virtual_len(), 0);
}

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

#[test]
fn single_contiguous_region_identity() {
    // Region starting at real offset 0 — identity mapping.
    let map = SpanMap::new(&[(0, 10)]);
    assert_eq!(map.virtual_len(), 10);
    assert_eq!(map.to_real(0), 0);
    assert_eq!(map.to_real(5), 5);
    assert_eq!(map.to_real(9), 9);
}

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

#[test]
fn remap_range_within_single_region() {
    let map = SpanMap::new(&[(10, 20)]);
    assert_eq!(map.remap_range(5..15), 15..25);
}

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

#[test]
fn remap_range_empty_is_identity_at_start() {
    let map = SpanMap::new(&[(100, 10)]);
    // Empty range: start == end → maps start, end = start
    assert_eq!(map.remap_range(5..5), 105..105);
}

#[test]
fn remap_fragment_basic() {
    let map = SpanMap::new(&[(100, 50)]);

    let frag = code_fragment(5..20, 0..25, 5, None, None, vec![]);

    let remapped = map.remap_fragment(frag);
    assert_eq!(remapped.byte_range, 105..120);
    assert_eq!(remapped.full_span, 100..125);
    assert_eq!(remapped.name_byte_offset, 105);
}

#[test]
fn remap_fragment_with_doc_and_decorator_ranges() {
    let map = SpanMap::new(&[(100, 50)]);

    let frag = code_fragment(10..30, 0..30, 10, Some(0..10), Some(5..10), vec![]);

    let remapped = map.remap_fragment(frag);
    assert_eq!(remapped.byte_range, 110..130);
    assert_eq!(remapped.full_span, 100..130);
    assert_eq!(remapped.name_byte_offset, 110);

    let FragmentMetadata::Code {
        doc_comment_range,
        decorator_range,
        ..
    } = &remapped.metadata
    else {
        panic!("expected Code metadata");
    };
    assert_eq!(doc_comment_range.as_ref(), Some(&(100..110)));
    assert_eq!(decorator_range.as_ref(), Some(&(105..110)));
}

#[test]
fn remap_fragment_recursive_children() {
    let map = SpanMap::new(&[(200, 100)]);

    let child = code_fragment(20..40, 15..45, 20, Some(15..20), None, vec![]);
    let parent = code_fragment(0..80, 0..80, 0, None, None, vec![child]);

    let remapped = map.remap_fragment(parent);
    assert_eq!(remapped.byte_range, 200..280);
    assert_eq!(remapped.name_byte_offset, 200);

    assert_eq!(remapped.children.len(), 1);
    let child = &remapped.children[0];
    assert_eq!(child.byte_range, 220..240);
    assert_eq!(child.full_span, 215..245);
    assert_eq!(child.name_byte_offset, 220);

    let FragmentMetadata::Code { doc_comment_range, .. } = &child.metadata else {
        panic!("expected Code metadata");
    };
    assert_eq!(doc_comment_range.as_ref(), Some(&(215..220)));
}

#[test]
fn remap_fragment_preserves_non_byte_fields() {
    let map = SpanMap::new(&[(50, 30)]);

    let mut frag = code_fragment(0..10, 0..10, 0, None, None, vec![]);
    frag.name = "my_func".to_owned();
    frag.signature = Some("fn my_func()".to_owned());
    frag.parent_name = Some("parent".to_owned());
    frag.fs_name = Some("my_func".to_owned());

    let remapped = map.remap_fragment(frag);
    assert_eq!(remapped.name, "my_func");
    assert_eq!(remapped.signature.as_deref(), Some("fn my_func()"));
    assert_eq!(remapped.parent_name.as_deref(), Some("parent"));
    assert_eq!(remapped.fs_name.as_deref(), Some("my_func"));
    // line_range is preserved as-is (caller recomputes from real source)
    assert_eq!(remapped.line_range, 0..1);
}

#[test]
fn remap_fragment_section_metadata_unchanged() {
    let map = SpanMap::new(&[(100, 50)]);

    let frag = Fragment {
        name: "section".to_owned(),
        kind: FragmentKind::Section { level: 2 },
        byte_range: 0..20,
        full_span: 0..20,
        line_range: 0..1,
        signature: None,
        metadata: FragmentMetadata::Document { index: 3 },
        name_byte_offset: 0,
        children: vec![],
        parent_name: None,
        fs_name: None,
    };

    let remapped = map.remap_fragment(frag);
    assert_eq!(remapped.byte_range, 100..120);
    assert_eq!(remapped.full_span, 100..120);
    assert_eq!(remapped.metadata, FragmentMetadata::Document { index: 3 });
}

#[test]
fn remap_fragment_code_block_metadata_unchanged() {
    let map = SpanMap::new(&[(50, 30)]);

    let frag = Fragment {
        name: "code_block".to_owned(),
        kind: FragmentKind::CodeBlock {
            lang: Some("rust".to_owned()),
        },
        byte_range: 5..15,
        full_span: 5..15,
        line_range: 0..1,
        signature: None,
        metadata: FragmentMetadata::CodeBlock { index: 1 },
        name_byte_offset: 5,
        children: vec![],
        parent_name: None,
        fs_name: None,
    };

    let remapped = map.remap_fragment(frag);
    assert_eq!(remapped.byte_range, 55..65);
    assert_eq!(remapped.metadata, FragmentMetadata::CodeBlock { index: 1 });
    assert_eq!(remapped.kind, FragmentKind::CodeBlock {
        lang: Some("rust".to_owned())
    });
}

#[test]
fn empty_map() {
    let map = SpanMap::new(&[]);
    assert_eq!(map.virtual_len(), 0);
}

#[test]
fn zero_length_regions_skipped() {
    let map = SpanMap::new(&[(10, 0), (20, 5)]);
    assert_eq!(map.virtual_len(), 5);
    assert_eq!(map.to_real(0), 20);
    assert_eq!(map.to_real(4), 24);
}

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
