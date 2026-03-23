use rstest::rstest;

use super::*;
use crate::syntax::span_map::SpanMap;

// Helper

/// Extract template and return (regions, symbols) for concise test setup.
fn extract(source: &str) -> (Vec<Range<usize>>, Vec<Jinja2Symbol>) {
    let t = extract_template(source);
    (t.regions, t.symbols)
}

/// Collect the text of all content regions, concatenated.
fn region_text(source: &str, regions: &[Range<usize>]) -> String {
    regions.iter().map(|r| &source[r.clone()]).collect()
}

// Content region extraction

#[test]
fn content_between_block_tags() {
    let source = "{% block title %}Hello world{% endblock %}";
    let (regions, _) = extract(source);

    assert_eq!(regions.len(), 1);
    assert_eq!(&source[regions[0].clone()], "Hello world");
    // Exact byte offsets: "{% block title %}" is 17 bytes.
    assert_eq!(regions[0], 17..28);
}

#[test]
fn no_content_between_adjacent_controls() {
    let source = "{% if true %}{% endif %}";
    let (regions, _) = extract(source);

    assert!(regions.is_empty(), "no content nodes between adjacent controls");
}

#[test]
fn leading_content_before_first_directive() {
    let source = "Hello\n{% block title %}world{% endblock %}";
    let (regions, _) = extract(source);

    assert_eq!(&source[regions[0].clone()], "Hello\n");
    assert_eq!(regions[0], 0..6);
}

#[test]
fn trailing_content_after_last_directive() {
    let source = "{% block title %}Hello{% endblock %}\nGoodbye";
    let (regions, _) = extract(source);

    let last = regions.last().expect("should have trailing region");
    assert_eq!(&source[last.clone()], "\nGoodbye");
}

#[test]
fn multiple_content_regions_all_captured() {
    //                     0         1         2         3         4
    //                     0123456789012345678901234567890123456789012345678
    let source = "Header\n{% block a %}Alpha{% endblock %}\nMiddle\n{% block b %}Beta{% endblock %}\nFooter";
    let (regions, _) = extract(source);

    let all = region_text(source, &regions);
    assert_eq!(all, "Header\nAlpha\nMiddle\nBeta\nFooter");
}

#[test]
fn render_expressions_are_not_content() {
    let source = "before{{ name }}after";
    let (regions, _) = extract(source);

    let all = region_text(source, &regions);
    assert_eq!(all, "beforeafter");
    assert_eq!(regions.len(), 2);
}

// Structural symbol extraction — blocks

#[test]
fn block_name_and_full_span() {
    let source = "{% block title %}Hello{% endblock %}";
    let (_, symbols) = extract(source);

    assert_eq!(symbols.len(), 1);
    let block = &symbols[0];
    assert_eq!(block.name, "title");
    assert_eq!(block.kind, SymbolKind::Module);
    assert_eq!(block.full_span, 0..source.len());
    assert_eq!(block.signature, "{% block title %}");
}

#[test]
fn block_name_byte_offset_points_to_identifier() {
    let source = "{% block title %}Hello{% endblock %}";
    let (_, symbols) = extract(source);

    let block = &symbols[0];
    // "{% block " is 9 bytes, so "title" starts at byte 9.
    assert_eq!(block.name_byte_offset, 9);
    assert_eq!(
        &source[block.name_byte_offset..block.name_byte_offset + block.name.len()],
        "title"
    );
}

#[test]
fn nested_blocks_paired_correctly() {
    let source = "{% block outer %}\n{% block inner %}Hello{% endblock %}\n{% endblock %}";
    let (_, symbols) = extract(source);

    assert_eq!(symbols.len(), 2);

    // Stack-based: inner closes first, so it appears first in output.
    let inner = &symbols[0];
    let outer = &symbols[1];
    assert_eq!(inner.name, "inner");
    assert_eq!(outer.name, "outer");

    // Inner's span is strictly contained within outer's.
    assert!(inner.full_span.start > outer.full_span.start);
    assert!(inner.full_span.end < outer.full_span.end);

    // Outer spans the entire source.
    assert_eq!(outer.full_span, 0..source.len());
}

// Structural symbol extraction — macros

#[test]
fn macro_name_kind_and_signature() {
    let source = "{% macro render(items) %}\n<ul>{{ items }}</ul>\n{% endmacro %}";
    let (_, symbols) = extract(source);

    assert_eq!(symbols.len(), 1);
    let mac = &symbols[0];
    assert_eq!(mac.name, "render");
    assert_eq!(mac.kind, SymbolKind::Function);
    assert_eq!(mac.full_span, 0..source.len());
    assert_eq!(mac.signature, "{% macro render(items) %}");
}

#[test]
fn macro_name_byte_offset() {
    let source = "{% macro greet(name) %}Hi {{ name }}{% endmacro %}";
    let (_, symbols) = extract(source);

    let mac = &symbols[0];
    // "{% macro " is 9 bytes.
    assert_eq!(mac.name_byte_offset, 9);
    assert_eq!(
        &source[mac.name_byte_offset..mac.name_byte_offset + mac.name.len()],
        "greet"
    );
}

// Structural symbol extraction — set

#[test]
fn set_variable_extracted() {
    let source = r#"{% set greeting = "hello" %}"#;
    let (_, symbols) = extract(source);

    assert_eq!(symbols.len(), 1);
    let var = &symbols[0];
    assert_eq!(var.name, "greeting");
    assert_eq!(var.kind, SymbolKind::Variable);
    assert_eq!(var.full_span, 0..source.len());
}

#[test]
fn set_name_byte_offset() {
    let source = r#"{% set x = 42 %}"#;
    let (_, symbols) = extract(source);

    let var = &symbols[0];
    // "{% set " is 7 bytes.
    assert_eq!(var.name_byte_offset, 7);
    assert_eq!(
        &source[var.name_byte_offset..var.name_byte_offset + var.name.len()],
        "x"
    );
}

// Critical: for/if inside blocks must not corrupt the stack

#[rstest]
#[case::for_loop(
    "content",
    "{% block content %}\n{% for item in items %}\n{{ item }}\n{% endfor %}\n{% endblock %}"
)]
#[case::if_branch("sidebar", "{% block sidebar %}\n{% if user %}\nHello\n{% endif %}\n{% endblock %}")]
#[case::nested_for_and_if(
    "main",
    "{% block main %}\n{% for x in xs %}\n{% if x %}\n{{ x }}\n{% endif %}\n{% endfor %}\n{% endblock %}"
)]
#[case::else_and_elif(
    "content",
    "{% block content %}\n{% if admin %}\nAdmin\n{% elif user %}\nUser\n{% else %}\nGuest\n{% endif %}\n{% endblock %}"
)]
fn control_flow_inside_block_does_not_corrupt_stack(#[case] name: &str, #[case] source: &str) {
    let (_, symbols) = extract(source);

    // Only the block should produce a symbol, not the inner control flow.
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].name, name);
    assert_eq!(symbols[0].kind, SymbolKind::Module);
    assert_eq!(symbols[0].full_span, 0..source.len());
}

#[test]
fn empty_template() {
    let t = extract_template("");

    assert!(t.regions.is_empty());
    assert!(t.symbols.is_empty());
}

// Mixed: content regions + symbols from the same parse

#[test]
fn extract_template_returns_both() {
    let source = "Header\n{% block title %}Body{% endblock %}\nFooter";
    let t = extract_template(source);

    // Regions: "Header\n", "Body", "\nFooter"
    let all_content = region_text(source, &t.regions);
    assert_eq!(all_content, "Header\nBody\nFooter");

    // Symbols: one block.
    assert_eq!(t.symbols.len(), 1);
    assert_eq!(t.symbols[0].name, "title");
}

#[test]
fn complex_template_content_and_symbols() {
    let source = r#"{% extends "base.html" %}
{% set title = "Page" %}
{% block content %}
# Welcome
{% for item in items %}
- {{ item }}
{% endfor %}
{% endblock %}
"#;
    let t = extract_template(source);

    // Should find: preamble (extends) + set (title) + block (content).
    let names: Vec<_> = t.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"preamble"), "should find preamble for extends");
    assert!(names.contains(&"title"), "should find set title");
    assert!(names.contains(&"content"), "should find block content");

    // for loop should NOT produce a symbol.
    assert_eq!(t.symbols.len(), 3);

    // Content should include the markdown and list marker, but not the
    // template directives themselves.
    let content = region_text(source, &t.regions);
    assert!(content.contains("# Welcome"), "content should have markdown heading");
    assert!(content.contains("- "), "content should have list markers");
    assert!(!content.contains("{% block"), "content should not contain directives");
}

// SpanMap integration — content concatenation invariant

#[test]
fn content_regions_produce_valid_span_map() {
    let source = "{% block title %}Hello {{ name }}{% endblock %}\nTrailing";
    let t = extract_template(source);

    let (map, content) = SpanMap::build(source, &t.regions);

    assert_eq!(content.len(), map.virtual_len());

    // Verify each region round-trips.
    let mut offset = 0;
    for region in &t.regions {
        let original = &source[region.clone()];
        let mapped = &content[offset..offset + region.len()];
        assert_eq!(original, mapped, "region text must round-trip through SpanMap");
        offset += region.len();
    }
}

// Fragment conversion

#[test]
fn symbols_to_fragments_correct_kinds_and_names() {
    let source = "{% block title %}Hello{% endblock %}\n{% set x = 1 %}";
    let t = extract_template(source);
    let fragments = symbols_to_fragments(t.symbols, source);

    insta::assert_debug_snapshot!(fragments);
}

#[test]
fn fragment_byte_ranges_extract_correct_source() {
    let source = "{% block title %}Hello{% endblock %}";
    let t = extract_template(source);
    let fragments = symbols_to_fragments(t.symbols, source);

    let frag = &fragments[0];
    assert_eq!(&source[frag.full_span.clone()], source);
    assert_eq!(&source[frag.byte_range.clone()], source);
}

#[test]
fn fragment_name_byte_offset_extracts_name() {
    let source = "{% block header %}content{% endblock %}";
    let t = extract_template(source);
    let fragments = symbols_to_fragments(t.symbols, source);

    let frag = &fragments[0];
    let extracted_name = &source[frag.name_byte_offset..frag.name_byte_offset + frag.name.len()];
    assert_eq!(extracted_name, "header");
}



fn decompose_fixture(name: &str) -> Vec<super::Fragment> {
    let source = crate::test_support::load_fixture("syntax/languages/jinja2", name);
    let t = extract_template(&source);
    symbols_to_fragments(t.symbols, &source)
}

/// Fixture has: extends, import, set, 3 blocks (head, content, footer), 1 macro.
/// Expected: preamble (extends + import) + set + 3 blocks + 1 macro = 6 fragments.
#[test]
fn basic_fixture_fragment_count() {
    let fragments = decompose_fixture("basic.html.j2");
    assert_eq!(fragments.len(), 6);
}

#[test]
fn basic_fixture_fragment_names() {
    let fragments = decompose_fixture("basic.html.j2");
    let names: Vec<_> = fragments.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, &["preamble", "page_title", "head", "content", "render_link", "footer"]);
}

/// Preamble fragment captures extends and import directives.
#[test]
fn basic_fixture_preamble() {
    let fragments = decompose_fixture("basic.html.j2");
    let first = &fragments[0];
    assert_eq!(first.name, "preamble");
    assert_eq!(first.kind, super::FragmentKind::Preamble);
}

/// Block and macro fragments have no children (flat structure).
#[test]
fn basic_fixture_no_children() {
    let fragments = decompose_fixture("basic.html.j2");
    for frag in &fragments {
        assert!(frag.children.is_empty(), "fragment '{}' should have no children", frag.name);
    }
}
