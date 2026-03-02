use rstest::rstest;

use crate::syntax::fragment::{DEFAULT_MAX_DEPTH, DecomposedFile, FragmentKind, SymbolKind};
use crate::test_support::registry;

fn load_fixture(name: &str) -> String { crate::test_support::load_fixture("syntax/injection", name) }

/// Decompose a `.j2` compound source through the registry.
fn decompose_j2(inner_ext: &str, source: &str) -> DecomposedFile {
    let reg = registry();
    let decomposer = reg
        .get_compound(inner_ext, "j2")
        .unwrap_or_else(|| panic!("no compound decomposer for ({inner_ext}, j2)"));
    let (file, _tree) = decomposer.decompose(source, DEFAULT_MAX_DEPTH);
    file
}

// Core pipeline

#[test]
fn markdown_in_jinja2_produces_both_layers() {
    let source = load_fixture("markdown-blocks.md.j2");
    let file = decompose_j2("md", &source);
    insta::assert_debug_snapshot!(file.fragments);
}

#[test]
fn toml_in_jinja2_decomposes_tables() {
    let source = load_fixture("toml-conditional.toml.j2");
    let file = decompose_j2("toml", &source);
    insta::assert_debug_snapshot!(file.fragments);
}

// Byte-range accuracy

#[test]
fn byte_ranges_point_to_original_source() {
    let source = load_fixture("markdown-header-block.md.j2");
    let file = decompose_j2("md", &source);
    for frag in &file.fragments {
        let extracted = &source[frag.full_span.clone()];
        assert!(
            !extracted.is_empty(),
            "fragment '{}' has empty full_span extraction",
            frag.name
        );
        if frag.name == "header" && matches!(frag.kind, FragmentKind::Symbol(SymbolKind::Module)) {
            assert!(
                extracted.contains("{% block header %}"),
                "block span should include opening tag, got: {extracted:?}"
            );
        }
    }
}

#[test]
fn inner_fragment_byte_ranges_round_trip() {
    let source = load_fixture("markdown-with-extends.md.j2");
    let file = decompose_j2("md", &source);
    let md_fragments: Vec<_> = file
        .fragments
        .iter()
        .filter(|f| matches!(f.kind, FragmentKind::Section { .. }))
        .collect();
    assert!(!md_fragments.is_empty(), "expected markdown section fragments");
    for frag in &md_fragments {
        let extracted = &source[frag.full_span.clone()];
        assert!(
            extracted.contains(&frag.name),
            "fragment '{}' full_span should contain its name in original source, got: {extracted:?}",
            frag.name
        );
    }
}

#[test]
fn multi_region_remapping_across_disjoint_gaps() {
    let source = load_fixture("multi-region.md.j2");
    let file = decompose_j2("md", &source);
    let sections: Vec<_> = file
        .fragments
        .iter()
        .filter(|f| matches!(f.kind, FragmentKind::Section { .. }))
        .collect();
    let section_names: Vec<_> = sections.iter().map(|f| f.name.as_str()).collect();
    assert!(
        section_names.contains(&"Header Section"),
        "missing header: {section_names:?}"
    );
    assert!(
        section_names.contains(&"Body Section"),
        "missing body: {section_names:?}"
    );
    assert!(
        section_names.contains(&"Footer Section"),
        "missing footer: {section_names:?}"
    );
    for section in &sections {
        let extracted = &source[section.full_span.clone()];
        assert!(
            extracted.contains(&section.name),
            "section '{}' full_span extracts wrong text: {extracted:?}",
            section.name
        );
    }
}

// Children & structure preservation

#[test]
fn preserves_inner_children() {
    let source = load_fixture("toml-block.toml.j2");
    let file = decompose_j2("toml", &source);
    insta::assert_debug_snapshot!(file.fragments);
}

#[test]
fn inner_decomposition_fields_pass_through() {
    use crate::syntax::languages::jinja2::extract_template;
    use crate::syntax::span_map::SpanMap;

    let source = load_fixture("toml-block.toml.j2");
    let reg = registry();
    let injection = reg.get_compound("toml", "j2").unwrap();
    let inner = reg.get("toml").unwrap();

    // Derive inner content from the fixture via the same extraction pipeline
    // the decomposer uses — no hardcoded strings that could drift from the fixture.
    let extraction = extract_template(&source);
    let (_map, inner_content) = SpanMap::build(&source, &extraction.regions);

    let (direct, _tree) = inner.decompose(&inner_content, DEFAULT_MAX_DEPTH);
    let (compound, _tree) = injection.decompose(&source, DEFAULT_MAX_DEPTH);

    assert_eq!(
        compound.imports.is_some(),
        direct.imports.is_some(),
        "imports pass-through mismatch"
    );
    assert_eq!(compound.file_doc, direct.file_doc, "file_doc pass-through mismatch");
}

// Line ranges & ordering

#[test]
fn line_ranges_are_correct_in_original() {
    use nyne::types::line_of_byte;

    let source = load_fixture("two-blocks.md.j2");
    let file = decompose_j2("md", &source);
    for frag in &file.fragments {
        let expected_start = line_of_byte(&source, frag.full_span.start);
        let expected_end = line_of_byte(&source, frag.full_span.end) + 1;
        assert_eq!(
            frag.line_range.start, expected_start,
            "fragment '{}' line_range.start mismatch",
            frag.name
        );
        assert_eq!(
            frag.line_range.end, expected_end,
            "fragment '{}' line_range.end mismatch",
            frag.name
        );
    }
}

#[test]
fn fragments_sorted_by_position() {
    let source = load_fixture("two-blocks.md.j2");
    let file = decompose_j2("md", &source);
    let positions: Vec<_> = file.fragments.iter().map(|f| f.full_span.start).collect();
    let mut sorted = positions.clone();
    sorted.sort_unstable();
    assert_eq!(positions, sorted, "fragments should be sorted by full_span.start");
}

// Edge cases

#[test]
fn empty_content_only_jinja2_directives() {
    let source = load_fixture("directives-only.md.j2");
    let file = decompose_j2("md", &source);
    insta::assert_debug_snapshot!(file.fragments);
}

#[test]
fn no_jinja2_directives_degenerates_to_inner_only() {
    let source = load_fixture("plain-content.md");
    let file = decompose_j2("md", &source);
    let (direct, _tree) = registry().get("md").unwrap().decompose(&source, DEFAULT_MAX_DEPTH);
    assert_eq!(
        file.fragments.len(),
        direct.fragments.len(),
        "injection with no directives should match direct decomposition"
    );
}

#[test]
fn empty_file_produces_no_fragments() {
    let source = load_fixture("empty.md.j2");
    let file = decompose_j2("md", &source);
    assert!(file.fragments.is_empty(), "empty file should produce no fragments");
}

// Invalid / malformed inputs — all must not panic and produce valid byte ranges

#[rstest]
#[case::unclosed_block("unclosed-block.md.j2")]
#[case::malformed_directive("malformed-directive.md.j2")]
#[case::nested_unclosed("nested-unclosed.md.j2")]
fn malformed_input_does_not_panic(#[case] fixture: &str) {
    let source = load_fixture(fixture);
    let file = decompose_j2("md", &source);
    for frag in &file.fragments {
        assert!(
            frag.full_span.end <= source.len(),
            "fragment '{}' full_span exceeds source length",
            frag.name
        );
    }
}

#[test]
fn unclosed_block_skips_unpaired_but_decomposes_content() {
    let source = load_fixture("unclosed-block.md.j2");
    let file = decompose_j2("md", &source);
    // Unclosed block should not appear as a structural symbol (no endblock
    // to pair with), but inner content should still decompose.
    let blocks: Vec<_> = file
        .fragments
        .iter()
        .filter(|f| matches!(f.kind, FragmentKind::Symbol(SymbolKind::Module)))
        .collect();
    assert!(blocks.is_empty(), "unclosed block should not produce a Module fragment");
    let sections: Vec<_> = file
        .fragments
        .iter()
        .filter(|f| matches!(f.kind, FragmentKind::Section { .. }))
        .collect();
    assert!(
        !sections.is_empty(),
        "inner content should still decompose despite unclosed block"
    );
}

// Validation

#[rstest]
#[case::valid_template("valid-template.md.j2")]
#[case::plain_content_fallback("plain-content.md")]
fn validate_accepts(#[case] fixture: &str) {
    let source = load_fixture(fixture);
    let reg = registry();
    let decomposer = reg.get_compound("md", "j2").unwrap();
    assert!(decomposer.validate(&source).is_ok());
}

#[test]
fn inner_full_span_does_not_extend_into_jinja2_directives() {
    let source = load_fixture("toml-block.toml.j2");
    let file = decompose_j2("toml", &source);

    // The `server` table is an inner (TOML) fragment whose full_span must
    // stay within the content region — it must NOT bleed into the
    // `{% endblock %}` directive that follows.
    let server = file
        .fragments
        .iter()
        .find(|f| f.name == "server")
        .expect("expected 'server' fragment");

    let extracted = &source[server.full_span.clone()];
    assert!(
        !extracted.contains("endblock"),
        "inner fragment full_span bleeds into Jinja2 directive: {extracted:?}"
    );
    assert!(
        extracted.contains("[server]"),
        "inner fragment full_span should contain the table header: {extracted:?}"
    );
}

#[test]
fn inner_full_span_does_not_extend_into_render_expressions() {
    // Render expressions ({{ var }}) create gaps in content regions just
    // like control directives. Inner fragment spans must not bleed into them.
    let source = load_fixture("render-expression.md.j2");
    let file = decompose_j2("md", &source);

    for frag in &file.fragments {
        let extracted = &source[frag.full_span.clone()];
        assert!(
            !extracted.contains("{{ variable }}"),
            "fragment '{}' full_span bleeds into render expression: {extracted:?}",
            frag.name
        );
    }
}
