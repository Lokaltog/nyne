use rstest::{fixture, rstest};

use super::fragment::{DEFAULT_MAX_DEPTH, DecomposedFile, Fragment, FragmentKind, SymbolKind};
use super::fs_mapping::slugify;
use crate::test_support::{registry, vfs};

/// Decompose source into fragments.
fn decompose(ext: &str, source: &str) -> DecomposedFile {
    let reg = registry();
    let d = reg.get(ext).unwrap();
    let (result, _tree) = d.decompose(source, DEFAULT_MAX_DEPTH);
    result
}

/// Decompose source and apply fs_name mapping.
fn decompose_mapped(ext: &str, source: &str) -> Vec<Fragment> {
    let reg = registry();
    let d = reg.get(ext).unwrap();
    let (mut result, _tree) = d.decompose(source, DEFAULT_MAX_DEPTH);
    d.map_to_fs(&mut result);
    result
}

// Registry

#[test]
fn registry_registers_all_extensions() {
    let reg = registry();
    let exts = reg.extensions();
    assert!(exts.contains(&"rs"), "missing rs");
    assert!(exts.contains(&"py"), "missing py");
    assert!(exts.contains(&"ts"), "missing ts");
    assert!(exts.contains(&"tsx"), "missing tsx");
    assert!(exts.contains(&"md"), "missing md");
    assert!(exts.contains(&"mdx"), "missing mdx");
}

// Snapshot: decompose tests

#[test]
fn rust_decompose_function() {
    insta::assert_debug_snapshot!(decompose("rs", "pub fn hello() {}\n"));
}

#[test]
fn rust_decompose_struct_with_impl() {
    let source = "\
pub struct Foo {
    x: i32,
}

impl Foo {
    pub fn new() -> Self {
        Self { x: 0 }
    }
}
";
    insta::assert_debug_snapshot!(decompose("rs", source));
}

#[test]
fn python_decompose_function_and_class() {
    let source = "\
def greet(name):
    \"\"\"Greet someone.\"\"\"
    print(f'Hello, {name}!')

class Person:
    def __init__(self, name):
        self.name = name
";
    insta::assert_debug_snapshot!(decompose("py", source));
}

#[test]
fn typescript_decompose_function_and_interface() {
    let source = "\
export function greet(name: string): void {
    console.log(`Hello, ${name}!`);
}

interface Config {
    host: string;
    port: number;
}
";
    insta::assert_debug_snapshot!(decompose("ts", source));
}

#[test]
fn python_decorated_function() {
    insta::assert_debug_snapshot!(decompose("py", "@app.route('/')\ndef index():\n    pass\n"));
}

#[test]
fn python_module_variable() {
    insta::assert_debug_snapshot!(decompose("py", "MAX_SIZE = 100\n"));
}

#[test]
fn python_decorated_class() {
    insta::assert_debug_snapshot!(decompose("py", "@dataclass\nclass Point:\n    x: int\n    y: int\n"));
}

#[test]
fn typescript_const_variable() {
    insta::assert_debug_snapshot!(decompose("ts", "export const API_URL = 'https://example.com';\n"));
}

#[test]
fn tsx_uses_different_grammar() {
    insta::assert_debug_snapshot!(decompose(
        "tsx",
        "export function App() {\n    return <div>Hello</div>;\n}\n"
    ));
}

#[test]
fn typescript_class_with_methods() {
    insta::assert_debug_snapshot!(decompose("ts", "class Greeter {\n    greet() { return 'hi'; }\n}\n"));
}

#[test]
fn typescript_enum_declaration() {
    insta::assert_debug_snapshot!(decompose(
        "ts",
        "export enum Color {\n    Red,\n    Green,\n    Blue,\n}\n"
    ));
}

#[test]
fn markdown_decompose_headings() {
    let source = "\
# Getting Started

Some intro text.

## Installation

Install with cargo.

## Usage

Run the binary.

# API Reference

Details here.
";
    insta::assert_debug_snapshot!(decompose("md", source));
}

#[test]
fn markdown_fs_mapping_slugified() {
    let frags = decompose_mapped("md", "# Getting Started\n\nText.\n\n## Quick Setup\n\nMore.\n");
    insta::assert_debug_snapshot!(frags);
}

#[test]
fn rust_trait_impl_naming() {
    let source =
        "impl Display for Foo {\n    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { Ok(()) }\n}\n";
    insta::assert_debug_snapshot!(decompose("rs", source));
}

#[test]
fn rust_imports_extracted() {
    let result = decompose("rs", "use std::io;\nuse std::fmt;\n\nfn main() {}\n");
    insta::assert_debug_snapshot!(super::fragment::find_fragment_of_kind(&result, &FragmentKind::Imports));
}

// Splice regression

/// Regression: reading a markdown section body and writing it back must not
/// duplicate the next section's heading.
///
/// The read path in `resolve_fragment_dir` uses `full_span.end` directly for
/// `SpliceMode::Line` languages (including markdown). Previously it used
/// `line_end_of(full_span.end)`, which extended past the section boundary
/// into the next heading — causing duplicate headers on round-trip writes.
#[test]
fn markdown_section_body_roundtrip_does_not_duplicate_next_heading() {
    use crate::edit::splice::{line_start_of, splice_content};

    let source = "\
# Title

Intro text.

## Section A

Content A.

## Section B

Content B.
";
    let result = decompose("md", source);

    // Find "Section A" fragment.
    let title_frag = result.iter().find(|f| f.name == "Title").expect("should find Title");
    let section_a = title_frag
        .children
        .iter()
        .find(|f| f.name == "Section A")
        .expect("should find Section A");

    // Simulate the fixed read path: line_start_of(start)..full_span().end
    // (no line_end_of — that was the bug).
    let body_start = line_start_of(source, section_a.full_span().start);
    let body_end = section_a.full_span().end;
    let read_body = &source[body_start..body_end];

    // The read body must NOT include the next section's heading.
    assert!(
        !read_body.contains("## Section B"),
        "read body should not include next heading:\n{read_body}"
    );

    // Simulate write-back: splice the read content at the same range.
    let spliced = splice_content(source, body_start..body_end, read_body);

    // Round-trip must be identity — no duplicate headings.
    assert_eq!(source, spliced, "round-trip splice should be identity");
}

/// Regression (fixture-based): every section in a multi-section markdown file
/// must round-trip through read→write without duplicating any sibling heading.
///
/// Uses an external fixture with nested headings, code blocks, and subsections
/// to cover realistic document structure.
#[test]
fn markdown_fixture_all_sections_roundtrip_without_duplication() {
    use crate::edit::splice::{line_start_of, splice_validate_write};

    let fixture = include_str!("fixtures/markdown-sections.md");

    let result = decompose("md", fixture);
    let reg = registry();
    let decomposer = reg.get("md").unwrap();

    // Collect all section fragments (flat walk including nested children).
    fn collect_sections(frags: &[Fragment], out: &mut Vec<Fragment>) {
        for f in frags {
            if matches!(f.kind, FragmentKind::Section { .. }) {
                out.push(f.clone());
                collect_sections(&f.children, out);
            }
        }
    }
    let mut sections = Vec::new();
    collect_sections(&result, &mut sections);
    assert!(
        sections.len() >= 4,
        "fixture should have at least 4 sections, got {}",
        sections.len()
    );

    // For each section, simulate read (line_start_of..full_span().end) then
    // splice_validate_write back — the file should remain identical.
    for section in &sections {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.md");
        std::fs::write(&file_path, fixture).unwrap();

        let fs = nyne::types::OsFs::new(dir.path().to_path_buf());
        let vfs_path = vfs("test.md");

        let body_start = line_start_of(fixture, section.full_span().start);
        let body_end = section.full_span().end;
        let read_body = &fixture[body_start..body_end];

        splice_validate_write(&fs, &vfs_path, body_start..body_end, read_body, |spliced| {
            let (re_decomposed, _tree) = decomposer.decompose(spliced, DEFAULT_MAX_DEPTH);
            if re_decomposed.is_empty() && !result.is_empty() {
                return Err("re-decomposition lost all fragments".into());
            }
            Ok(())
        })
        .unwrap_or_else(|e| panic!("round-trip splice of {:?} failed: {e}", section.name));

        let written = std::fs::read_to_string(&file_path).unwrap();
        similar_asserts::assert_eq!(
            fixture,
            written,
            "round-trip of section {:?} should be identity",
            section.name
        );
    }
}

/// Regression: splicing a larger body into a nested child (e.g. a method inside
/// an impl block) must not corrupt content after the impl block.
///
/// Simulates the write path in `resolve_fragment_dir`: the body splice range
/// is `line_start_of(source, frag.full_span().start)..frag.full_span().end`.
#[test]
fn nested_child_body_splice_does_not_corrupt_trailing_content() {
    use crate::edit::splice::{line_start_of, splice_content};

    let source = "\
pub struct Foo {
    pub name: String,
}

impl Foo {
    pub fn new() -> Self {
        Self { name: String::new() }
    }
}

pub struct Bar {
    pub value: String,
}
";
    let result = decompose("rs", source);
    let impl_frag = result
        .iter()
        .find(|f| f.kind == FragmentKind::Symbol(SymbolKind::Impl))
        .expect("should find impl");
    let child = &impl_frag.children[0];
    assert_eq!(child.name, "new");

    let body_start = line_start_of(source, child.full_span().start);
    let body_range = body_start..child.full_span().end;
    let original_body = &source[body_range.clone()];

    let new_body =
        format!("    /// Construct a new Foo.\n    ///\n    /// This is an extended docstring.\n{original_body}");

    let spliced = splice_content(source, body_range, &new_body);

    assert!(
        spliced.contains("pub struct Bar {\n    pub value: String,\n}"),
        "trailing struct Bar was corrupted:\n{spliced}"
    );

    registry().get("rs").unwrap().validate(&spliced).unwrap_or_else(|e| {
        panic!("spliced result should be valid Rust: {e}\n---\n{spliced}");
    });
}

// Doc comment: unique tests

#[test]
fn rust_doc_comment_extracted() {
    let result = decompose("rs", "/// Does a thing.\npub fn thing() {}\n");
    assert_eq!(result.len(), 1);
    let doc = registry().get("rs").unwrap().clean_doc_comment("/// Does a thing.");
    assert_eq!(doc.as_deref(), Some("Does a thing."));
}

#[test]
fn rust_doc_comment_range_extends_fragment() {
    let source = "/// Doc line.\npub fn documented() {}\n";
    let result = decompose("rs", source);
    let frag = &result[0];

    assert_eq!(frag.byte_range.start, 14, "byte_range should start at fn keyword");
    assert_eq!(frag.full_span().start, 0, "full_span should include doc comment");
    assert_eq!(
        frag.line_range(source).start,
        0,
        "line_range should include doc comment"
    );
}

#[test]
fn python_body_internal_docstring_does_not_shrink_range() {
    let result = decompose("py", "class Foo:\n    \"\"\"Docstring.\"\"\"\n    x = 1\n");
    let frag = &result[0];

    assert_eq!(frag.byte_range.start, 0, "byte_range should start at class keyword");
    assert_eq!(frag.full_span().start, 0, "full_span should start at class keyword");
    assert_eq!(
        frag.full_span().end,
        frag.byte_range.end,
        "full_span should match byte_range for body-internal docstrings"
    );
}

// Group 1: No doc comment returns None

#[rstest]
#[case::rust_bare("rs", "fn bare() {}\n")]
#[case::rust_regular_comment("rs", "// Just a comment.\nfn foo() {}\n")]
#[case::python_bare("py", "def bare():\n    pass\n")]
#[case::typescript_bare("ts", "function bare() {}\n")]
fn no_doc_comment_returns_none(#[case] ext: &str, #[case] source: &str) {
    let result = decompose(ext, source);
    assert!(
        result[0].child_of_kind(&FragmentKind::Docstring).is_none(),
        "should have no doc comment child"
    );
}

// Group 2: No decorator returns None

#[rstest]
#[case::rust("rs", "fn bare() {}\n")]
#[case::python("py", "def bare():\n    pass\n")]
#[case::typescript("ts", "function bare() {}\n")]
fn no_decorator_returns_none(#[case] ext: &str, #[case] source: &str) {
    let result = decompose(ext, source);
    assert!(
        result[0].child_of_kind(&FragmentKind::Decorator).is_none(),
        "should have no decorator child"
    );
}

// Group 3: Doc comment range present with text checks

#[rstest]
#[case::rust_multi_line("rs", "/// Line one.\n/// Line two.\nfn foo() {}\n", &["Line one", "Line two"], &["fn foo"])]
#[case::python_function("py", "def greet():\n    \"\"\"Say hello.\"\"\"\n    pass\n", &["\"\"\"Say hello.\"\"\""], &[])]
#[case::python_class("py", "class Foo:\n    \"\"\"A foo.\"\"\"\n    x = 1\n", &["\"\"\"A foo.\"\"\""], &[])]
#[case::typescript_jsdoc("ts", "/** Does a thing. */\nfunction doThing() {}\n", &["Does a thing"], &["function"])]
fn doc_comment_range_covers_expected(
    #[case] ext: &str,
    #[case] source: &str,
    #[case] must_contain: &[&str],
    #[case] must_not_contain: &[&str],
) {
    let result = decompose(ext, source);
    let doc_child = result[0]
        .child_of_kind(&FragmentKind::Docstring)
        .expect("should have doc comment child");
    let text = &source[doc_child.byte_range.clone()];
    for &expected in must_contain {
        assert!(
            text.contains(expected),
            "range should contain {expected:?}, got {text:?}"
        );
    }
    for &unexpected in must_not_contain {
        assert!(
            !text.contains(unexpected),
            "range should NOT contain {unexpected:?}, got {text:?}"
        );
    }
}

// Group 4: Doc comment extraction is_some

#[rstest]
#[case::python_docstring("py", "def foo():\n    \"\"\"This is a docstring.\"\"\"\n    pass\n")]
#[case::typescript_jsdoc("ts", "/** Does a thing. */\nfunction doThing() {}\n")]
#[case::typescript_triple_slash("ts", "/// A triple-slash doc.\nfunction foo() {}\n")]
fn doc_comment_is_captured(#[case] ext: &str, #[case] source: &str) {
    let result = decompose(ext, source);
    assert!(
        result[0].child_of_kind(&FragmentKind::Docstring).is_some(),
        "doc comment should be captured"
    );
}

// Group 5: Decorator range present with text checks

#[rstest]
#[case::rust_single_attribute("rs", "#[derive(Debug)]\npub struct Foo;\n", &["derive(Debug)"], &["pub struct"])]
#[case::rust_multiple_attributes("rs", "#[derive(Debug)]\n#[serde(rename_all = \"camelCase\")]\npub struct Foo;\n", &["derive(Debug)", "serde"], &["pub struct"])]
#[case::python_single("py", "@app.route('/')\ndef index():\n    pass\n", &["@app.route"], &["def index"])]
#[case::python_multiple("py", "@staticmethod\n@cache\ndef compute():\n    pass\n", &["@staticmethod", "@cache"], &[])]
fn decorator_range_covers_expected(
    #[case] ext: &str,
    #[case] source: &str,
    #[case] must_contain: &[&str],
    #[case] must_not_contain: &[&str],
) {
    let result = decompose(ext, source);
    let dec_child = result[0]
        .child_of_kind(&FragmentKind::Decorator)
        .expect("should have decorator child");
    let text = &source[dec_child.byte_range.clone()];
    for &expected in must_contain {
        assert!(
            text.contains(expected),
            "range should contain {expected:?}, got {text:?}"
        );
    }
    for &unexpected in must_not_contain {
        assert!(
            !text.contains(unexpected),
            "range should NOT contain {unexpected:?}, got {text:?}"
        );
    }
}

// Group 6: File level doc

#[rstest]
#[case::rust(
    "rs",
    "//! Module-level doc.\n//! Second line.\n\nfn foo() {}\n",
    "Module-level doc."
)]
#[case::python("py", "\"\"\"Module docstring.\"\"\"\n\ndef foo():\n    pass\n", "Module docstring.")]
fn file_level_doc_extracted(#[case] ext: &str, #[case] source: &str, #[case] expected: &str) {
    let result = decompose(ext, source);
    let reg = registry();
    let d = reg.get(ext).unwrap();
    let doc_frag = super::fragment::find_fragment_of_kind(&result, &FragmentKind::Docstring)
        .expect("should have file-level docstring fragment");
    let raw = &source[doc_frag.byte_range.clone()];
    let cleaned = d.clean_doc_comment(raw).expect("should clean doc comment");
    assert_eq!(cleaned, expected);
}

#[rstest]
#[case::rust("rs", "//! Module-level doc.\n//! Second line.\n\nfn foo() {}\n")]
#[case::python("py", "\"\"\"Module docstring.\nSecond line.\n\"\"\"\n\ndef foo():\n    pass\n")]
fn file_level_doc_strip_wrap_roundtrip(#[case] ext: &str, #[case] source: &str) {
    let result = decompose(ext, source);
    let reg = registry();
    let d = reg.get(ext).unwrap();
    let doc_frag = super::fragment::find_fragment_of_kind(&result, &FragmentKind::Docstring)
        .expect("should have file-level docstring fragment");
    let raw = &source[doc_frag.byte_range.clone()];
    let stripped = d.strip_doc_comment(raw);
    let rewrapped = d.wrap_file_doc_comment(&stripped, "");
    assert_eq!(rewrapped, raw, "strip → wrap_file_doc_comment should round-trip");
}

// Group 7: Disjoint doc+decorator ranges

#[rstest]
#[case::rust("rs", "/// Documented.\n#[derive(Debug)]\npub struct Foo;\n")]
#[case::python("py", "@dataclass\nclass Cfg:\n    \"\"\"Config.\"\"\"\n    x: int = 0\n")]
fn doc_and_decorator_ranges_are_disjoint(#[case] ext: &str, #[case] source: &str) {
    let result = decompose(ext, source);
    let doc = result[0]
        .child_of_kind(&FragmentKind::Docstring)
        .expect("should have doc child");
    let dec = result[0]
        .child_of_kind(&FragmentKind::Decorator)
        .expect("should have decorator child");
    assert!(
        doc.byte_range.end <= dec.byte_range.start || dec.byte_range.end <= doc.byte_range.start,
        "ranges should not overlap"
    );
}

// Group 8: Line range symlink tests (byte_and_line_ranges_match)

#[rstest]
#[case::rust_top_level("rs", "fn foo() {}\n")]
#[case::rust_doc_comment("rs", "/// Documented.\nfn foo() {}\n")]
#[case::rust_attribute_and_doc("rs", "/// Documented.\n#[derive(Debug)]\npub struct Foo;\n")]
#[case::rust_multiple_symbols("rs", "fn alpha() {}\n\nfn beta() {\n    42\n}\n")]
#[case::rust_nested_methods("rs", "impl Foo {\n    fn bar() {}\n    fn baz() { 1 }\n}\n")]
#[case::python_decorated_class("py", "@dataclass\nclass Point:\n    x: int\n    y: int\n")]
#[case::python_docstring("py", "def greet():\n    \"\"\"Say hello.\"\"\"\n    pass\n")]
#[case::typescript_jsdoc("ts", "/** Does a thing. */\nfunction doThing() {}\n")]
fn byte_and_line_ranges_match(#[case] ext: &str, #[case] source: &str) {
    use nyne::types::SymbolLineRange;

    let result = decompose(ext, source);

    fn check_fragments(source: &str, fragments: &[Fragment]) {
        for frag in fragments {
            let from_line = SymbolLineRange::from_zero_based(&frag.line_range(source));
            let from_bytes = SymbolLineRange::from_byte_range(source, &frag.full_span());
            assert_eq!(
                from_line,
                from_bytes,
                "line_range mismatch for '{}': from_zero_based={from_line:?}, from_byte_range={from_bytes:?}, \
                 full_span={:?}, line_range={:?}",
                frag.name,
                frag.full_span(),
                frag.line_range(source),
            );
            check_fragments(source, &frag.children);
        }
    }
    check_fragments(source, &result);
}

// Line range: individual tests (from former line_range_symlink_tests submod)

#[test]
fn decorator_byte_range_to_line_range() {
    use nyne::types::SymbolLineRange;

    let source = "/// Doc.\n#[derive(Debug)]\npub struct Foo;\n";
    let result = decompose("rs", source);
    let dec_child = result[0]
        .child_of_kind(&FragmentKind::Decorator)
        .expect("should have decorator child");
    let line_range = SymbolLineRange::from_byte_range(source, &dec_child.byte_range);
    assert_eq!(line_range, SymbolLineRange { start: 2, end: 2 });
}

#[test]
fn import_span_line_range_matches_byte_range() {
    use nyne::types::SymbolLineRange;

    let source = "use std::io;\nuse std::fmt;\n\nfn foo() {}\n";
    let result = decompose("rs", source);
    let imports = super::fragment::find_fragment_of_kind(&result, &FragmentKind::Imports).expect("should have imports");
    let from_line = SymbolLineRange::from_zero_based(&imports.line_range(source));
    let from_bytes = SymbolLineRange::from_byte_range(source, &imports.byte_range);
    assert_eq!(
        from_line, from_bytes,
        "import line_range mismatch: from_zero_based={from_line:?}, from_byte_range={from_bytes:?}"
    );
}

#[test]
fn lines_suffix_roundtrips_through_parse() {
    use nyne::types::SymbolLineRange;

    use crate::edit::slice::parse_slice_suffix;

    let range = SymbolLineRange { start: 5, end: 10 };
    let suffix = range.as_lines_suffix();
    let (base, spec) = parse_slice_suffix(&suffix).expect("should parse");
    assert_eq!(base, "lines");
    assert_eq!(spec, crate::edit::slice::SliceSpec::Range(5, 10));

    let single = SymbolLineRange { start: 3, end: 3 };
    let suffix = single.as_lines_suffix();
    let (base, spec) = parse_slice_suffix(&suffix).expect("should parse");
    assert_eq!(base, "lines");
    assert_eq!(spec, crate::edit::slice::SliceSpec::Single(3));
}

// full_span tests (unique assertions per test)

#[test]
fn full_span_bare_symbol_matches_byte_range() {
    let result = decompose("rs", "fn bare() {}\n");
    let frag = &result[0];
    assert_eq!(
        frag.full_span(),
        frag.byte_range,
        "bare symbol: full_span == byte_range"
    );
}

#[test]
fn full_span_rust_decorator_only() {
    let result = decompose("rs", "#[derive(Debug)]\npub struct Foo;\n");
    let frag = &result[0];
    assert_eq!(frag.full_span().start, 0, "full_span should include attribute");
    assert!(frag.byte_range.start > 0, "byte_range should start after attribute");
    assert_eq!(frag.full_span().end, frag.byte_range.end, "ends should match");
}

#[test]
fn full_span_rust_decorator_and_doc_comment() {
    let result = decompose("rs", "/// Documented.\n#[derive(Debug)]\npub struct Foo;\n");
    let frag = &result[0];
    assert_eq!(frag.full_span().start, 0, "full_span should start at doc comment");
    assert!(frag.byte_range.start > 0, "byte_range should be the node range only");
    assert_eq!(frag.full_span().end, frag.byte_range.end);
}

#[test]
fn full_span_python_decorated_class_with_docstring() {
    let result = decompose("py", "@dataclass\nclass Bar:\n    \"\"\"A bar.\"\"\"\n    x: int = 0\n");
    let frag = &result[0];
    assert_eq!(frag.full_span().start, 0, "full_span should include decorator");
    assert_eq!(frag.byte_range.start, 0, "wrapper node includes decorator");
    assert_eq!(frag.full_span().end, frag.byte_range.end);
}

#[test]
fn full_span_typescript_jsdoc() {
    let result = decompose("ts", "/** Documented. */\nfunction greet() {}\n");
    let frag = &result[0];
    assert_eq!(frag.full_span().start, 0, "full_span should include JSDoc");
    assert!(frag.byte_range.start > 0, "byte_range should start at function keyword");
    assert_eq!(frag.full_span().end, frag.byte_range.end);
}

#[test]
fn full_span_python_multi_symbol_delete_scenario() {
    let source = "\
class First:
    \"\"\"First class.\"\"\"
    x = 1


class Second:
    pass
";
    let result = decompose("py", source);
    assert_eq!(result.len(), 2);

    let first = &result[0];
    let second = &result[1];

    assert_eq!(
        first.full_span().start,
        0,
        "first class full_span starts at class keyword"
    );
    assert_eq!(
        first.byte_range.start, 0,
        "first class byte_range starts at class keyword"
    );
    assert!(
        first.full_span().end <= second.full_span().start,
        "first.full_span().end ({}) must not overlap second.full_span().start ({})",
        first.full_span().end,
        second.full_span().start,
    );
}

// Strip / wrap doc comment tests

#[test]
fn rust_strip_and_wrap_doc_comment() {
    let reg = registry();
    let d = reg.get("rs").unwrap();
    let stripped = d.strip_doc_comment("/// Hello\n/// World");
    assert_eq!(stripped, "Hello\nWorld");
    let wrapped = d.wrap_doc_comment("Hello\nWorld", "    ");
    assert!(wrapped.starts_with("/// Hello"));
    assert!(wrapped.contains("    /// World"));
}

#[test]
fn python_strip_doc_comment_triple_quotes() {
    let stripped = registry()
        .get("py")
        .unwrap()
        .strip_doc_comment("\"\"\"Hello world.\"\"\"");
    assert_eq!(stripped, "Hello world.");
}

#[test]
fn typescript_strip_jsdoc() {
    let stripped = registry()
        .get("ts")
        .unwrap()
        .strip_doc_comment("/**\n * Hello\n * World\n */");
    assert_eq!(stripped, "Hello\nWorld");
}

// Visibility

#[test]
fn rust_visibility_extracted() {
    let result = decompose("rs", "pub(crate) fn internal() {}\n");
    let visibility = &result[0].visibility;
    assert_eq!(visibility.as_deref(), Some("pub(crate)"));
}

#[test]
fn typescript_exported_visibility() {
    let result = decompose("ts", "export function greet() {}\n");
    let visibility = &result[0].visibility;
    assert_eq!(visibility.as_deref(), Some("export"));
}

// Docstring splice round-trip

#[test]
fn rust_docstring_splice_round_trip() {
    let reg = registry();
    let d = reg.get("rs").unwrap();
    let source = "\
struct Foo;

/// Doc line one.
///
/// Doc line two.
fn bar() {}
";
    let result = decompose("rs", source);
    let frag = result.iter().find(|f| f.name == "bar").expect("should find bar");
    let doc_child = frag
        .child_of_kind(&FragmentKind::Docstring)
        .expect("should have doc comment child");
    let doc_range = &doc_child.byte_range;
    let raw_doc = &source[doc_range.clone()];

    let stripped = d.strip_doc_comment(raw_doc);
    let new_plain = format!("{stripped}\nAppended line.");
    let wrapped = d.wrap_doc_comment(&new_plain, "");

    let spliced = format!("{}{wrapped}{}", &source[..doc_range.start], &source[doc_range.end..]);

    assert!(
        spliced.contains("\nfn bar"),
        "fn must be on its own line after doc comment, got:\n{spliced}"
    );
    d.validate(&spliced)
        .unwrap_or_else(|e| panic!("spliced result should be valid Rust: {e}\n---\n{spliced}"));
}

// Decorator: typescript ignored test

#[test]
#[ignore = "TS decorator extraction only works on exported classes — needs unwrap_wrapper support"]
fn typescript_decorator_on_class() {
    let source = "@Injectable()\nclass Service {}\n";
    let result = decompose("ts", source);
    let dec_child = result[0]
        .child_of_kind(&FragmentKind::Decorator)
        .expect("TS decorator should be captured");
    let text = &source[dec_child.byte_range.clone()];
    assert!(
        text.contains("@Injectable"),
        "range should cover the decorator: got {text:?}"
    );
}

// FS mapping / conflict resolution

#[test]
fn identity_fs_mapping() {
    let frags = decompose_mapped("rs", "fn alpha() {}\nfn beta() {}\n");
    assert_eq!(frags[0].fs_name.as_deref(), Some("alpha"));
    assert_eq!(frags[1].fs_name.as_deref(), Some("beta"));
}

#[test]
fn structural_fragments_have_no_fs_name() {
    let source = "//! Module doc.\nuse std::io;\n\n/// Documented.\n#[derive(Debug)]\npub struct Foo;\n";
    let frags = decompose_mapped("rs", source);
    for frag in &frags {
        if frag.kind.is_structural() {
            assert!(
                frag.fs_name.is_none(),
                "{:?} fragment {:?} should not have fs_name, got {:?}",
                frag.kind,
                frag.name,
                frag.fs_name,
            );
        }
    }
    // Verify the structural fragments exist but are nameless.
    let file_doc = super::fragment::find_fragment_of_kind(&frags, &FragmentKind::Docstring);
    assert!(file_doc.is_some(), "should have file-level docstring fragment");
    assert!(file_doc.unwrap().fs_name.is_none());

    let imports = super::fragment::find_fragment_of_kind(&frags, &FragmentKind::Imports);
    assert!(imports.is_some(), "should have imports fragment");
    assert!(imports.unwrap().fs_name.is_none());

    // Symbol's child docstring and decorator should also be nameless.
    let foo = frags.iter().find(|f| f.name == "Foo").expect("Foo symbol");
    assert!(foo.fs_name.is_some(), "symbol should have fs_name");
    let doc_child = foo.child_of_kind(&FragmentKind::Docstring).unwrap();
    assert!(doc_child.fs_name.is_none(), "docstring child should not have fs_name");
    let dec_child = foo.child_of_kind(&FragmentKind::Decorator).unwrap();
    assert!(dec_child.fs_name.is_none(), "decorator child should not have fs_name");
}

#[test]
fn kind_suffix_conflict_resolution() {
    let reg = registry();
    let d = reg.get("rs").unwrap();
    let mut result = decompose("rs", "struct Foo;\nfn Foo() {}\n");
    d.map_to_fs(&mut result);

    use super::fragment::{ConflictEntry, ConflictSet};
    let conflicts = vec![ConflictSet {
        name: "Foo".to_owned(),
        entries: vec![
            ConflictEntry {
                index: 0,
                fragment_name: "Foo".to_owned(),
                fragment_kind: result[0].kind.clone(),
            },
            ConflictEntry {
                index: 1,
                fragment_name: "Foo".to_owned(),
                fragment_kind: result[1].kind.clone(),
            },
        ],
    }];
    let resolutions = d.resolve_conflicts(&conflicts);
    assert_eq!(resolutions.len(), 2);
    assert_eq!(resolutions[0].fs_name.as_deref(), Some("Foo~Struct"));
    assert_eq!(resolutions[1].fs_name.as_deref(), Some("Foo~Function"));
}

#[test]
fn numbered_conflict_resolution() {
    let reg = registry();
    let d = reg.get("md").unwrap();
    let mut result = decompose("md", "# Intro\n\n# Intro\n\n# Intro\n");
    d.map_to_fs(&mut result);

    use super::fragment::{ConflictEntry, ConflictSet};
    let conflicts = vec![ConflictSet {
        name: result[0].fs_name.clone().unwrap(),
        entries: result
            .iter()
            .enumerate()
            .map(|(i, f)| ConflictEntry {
                index: i,
                fragment_name: f.name.clone(),
                fragment_kind: f.kind.clone(),
            })
            .collect(),
    }];
    let resolutions = d.resolve_conflicts(&conflicts);
    assert_eq!(resolutions[0].fs_name.as_deref(), Some("00-intro"));
    assert_eq!(resolutions[1].fs_name.as_deref(), Some("00-intro-2"));
    assert_eq!(resolutions[2].fs_name.as_deref(), Some("00-intro-3"));
}

#[test]
fn slugify_basic() {
    assert_eq!(slugify("Getting Started"), "getting-started");
    assert_eq!(slugify("API Reference"), "api-reference");
    assert_eq!(slugify("  leading & trailing  "), "leading-trailing");
    assert_eq!(slugify("CamelCase"), "camel-case");
}

// Validation

#[test]
fn validate_valid_source() {
    assert!(registry().get("rs").unwrap().validate("fn foo() {}\n").is_ok());
}

#[test]
fn validate_invalid_source() {
    assert!(registry().get("rs").unwrap().validate("fn foo( {}\n").is_err());
}

// extract_symbol

#[test]
fn extract_symbol_top_level() {
    let reg = registry();
    let source = "fn hello() {}\n\nfn world() { 42 }\n";
    let result = reg.extract_symbol(source, "rs", &["world".into()]);
    assert_eq!(result.as_deref(), Some("fn world() { 42 }"));
}

#[test]
fn extract_symbol_nested() {
    let reg = registry();
    let source = "impl Foo {\n    fn bar() {}\n    fn baz() {}\n}\n";
    let body = reg.extract_symbol(source, "rs", &["Foo".into(), "baz".into()]).unwrap();
    assert_eq!(body.trim(), "fn baz() {}");
}

#[test]
fn extract_symbol_missing_returns_none() {
    let reg = registry();
    let source = "fn hello() {}\n";
    assert!(reg.extract_symbol(source, "rs", &["nonexistent".into()]).is_none());
}

#[test]
fn extract_symbol_unknown_ext_returns_none() {
    let reg = registry();
    assert!(reg.extract_symbol("some content", "xyz", &["foo".into()]).is_none());
}

// Registry compound lookup

#[test]
fn registry_compound_lookup_returns_decomposer() {
    let reg = registry();
    assert!(reg.get_compound("md", "j2").is_some(), "missing compound md+j2");
    assert!(reg.get_compound("toml", "j2").is_some(), "missing compound toml+j2");
    assert!(reg.get_compound("rs", "j2").is_some(), "missing compound rs+j2");
}

#[test]
fn registry_compound_unknown_inner_returns_none() {
    let reg = registry();
    assert!(reg.get_compound("xyz", "j2").is_none());
}

#[test]
fn registry_compound_unknown_outer_returns_none() {
    let reg = registry();
    assert!(reg.get_compound("rs", "gz").is_none());
}

#[test]
fn registry_simple_lookup_unaffected_by_compound() {
    let reg = registry();
    assert!(reg.get("rs").is_some());
    assert!(reg.get("py").is_some());
    assert!(reg.get("md").is_some());
}

#[test]
fn registry_has_all_j2_compounds() {
    let reg = registry();
    for ext in reg.extensions() {
        assert!(reg.get_compound(ext, "j2").is_some(), "missing compound {ext}+j2");
    }
}

#[test]
fn decomposer_for_simple_extension() {
    let reg = registry();
    let path = vfs("src/main.rs");
    assert!(
        reg.decomposer_for(&path).is_some(),
        "decomposer_for should find simple .rs decomposer"
    );
}

#[test]
fn decomposer_for_compound_j2_extension() {
    let reg = registry();
    let path = vfs("templates/page.md.j2");
    assert!(
        reg.decomposer_for(&path).is_some(),
        "decomposer_for should find compound .md.j2 decomposer"
    );
}

#[test]
fn decomposer_for_unknown_extension_returns_none() {
    let reg = registry();
    let path = vfs("data/file.xyz");
    assert!(reg.decomposer_for(&path).is_none());
}

#[rstest]
#[case::md_j2("templates/page.md.j2")]
#[case::toml_j2("config/app.toml.j2")]
#[case::py_j2("scripts/build.py.j2")]
fn decomposer_for_compound_decomposes_j2_content(#[case] path_str: &str) {
    let reg = registry();
    let path = vfs(path_str);
    let decomposer = reg.decomposer_for(&path).expect("compound decomposer should exist");

    let source = "{% block content %}\nHello\n{% endblock %}";
    let (result, _tree) = decomposer.decompose(source, DEFAULT_MAX_DEPTH);
    assert!(
        !result.is_empty(),
        "compound decomposer for {path_str} should produce fragments"
    );
}

// find_fragment_at_line / find_nearest_fragment_at_line

/// Source: `use std::io; / fn alpha() {} / fn beta() {}`
/// Lines (0-based): 0=import, 1=blank, 2=alpha, 3=blank, 4=beta
#[fixture]
fn alpha_beta_fragments() -> (Vec<Fragment>, String) {
    let src = "use std::io;\n\nfn alpha() {}\n\nfn beta() {}\n";
    (decompose_mapped("rs", src), src.to_owned())
}

/// Source: `struct Foo {} / impl Foo { fn bar() {} } / impl Foo { fn baz() {} }`
///
/// Two inherent impl blocks trigger KindSuffix conflict: `Foo~Impl` appears
/// twice → non-unique → conflict resolution hides both impls (`fs_name = None`).
///
/// Lines (0-based): 0=struct, 2=impl#1, 3=bar, 6=impl#2, 7=baz
#[fixture]
fn nameless_impl_fragments() -> (Vec<Fragment>, String) {
    let src = "struct Foo {}\n\nimpl Foo {\n    fn bar() {}\n}\n\nimpl Foo {\n    fn baz() {}\n}\n";
    let mut frags = decompose_mapped("rs", src);
    // Simulate conflict resolution hiding inherent impl blocks.
    for frag in &mut frags {
        if frag.kind == FragmentKind::Symbol(SymbolKind::Impl) && frag.name == "Foo" {
            frag.fs_name = None;
        }
    }
    (frags, src.to_owned())
}

#[rstest]
#[case::exact_match(2, &["alpha"])]
#[case::gap_before_first_symbol(0, &["alpha"])]
#[case::gap_between_symbols(3, &["alpha"])]
fn nearest_fragment_at_line(
    alpha_beta_fragments: (Vec<Fragment>, String),
    #[case] line: usize,
    #[case] expected: &[&str],
) {
    let (frags, source) = alpha_beta_fragments;
    let path = super::find_nearest_fragment_at_line(&frags, line, &source);
    let expected: Vec<String> = expected.iter().map(|s| (*s).to_owned()).collect();
    assert_eq!(path.as_deref(), Some(expected.as_slice()));
}

#[test]
fn nearest_fragment_at_line_empty_fragments_returns_none() {
    assert_eq!(super::find_nearest_fragment_at_line(&[], 5, ""), None);
}

#[rstest]
fn fragment_at_line_inside_nameless_parent(nameless_impl_fragments: (Vec<Fragment>, String)) {
    let (frags, source) = nameless_impl_fragments;
    // Line 3 is inside bar, child of a nameless impl — should find bar.
    let path = super::find_fragment_at_line(&frags, 3, &source);
    assert_eq!(path.as_deref(), Some(&["bar".to_owned()][..]));
}

#[rstest]
#[case::exact_child(3, &["bar"])]
#[case::gap_in_nameless_parent(2, &["bar"])]
fn nearest_fragment_at_line_nameless_parent(
    nameless_impl_fragments: (Vec<Fragment>, String),
    #[case] line: usize,
    #[case] expected: &[&str],
) {
    let (frags, source) = nameless_impl_fragments;
    let path = super::find_nearest_fragment_at_line(&frags, line, &source);
    let expected: Vec<String> = expected.iter().map(|s| (*s).to_owned()).collect();
    assert_eq!(path.as_deref(), Some(expected.as_slice()));
}

// Property-based tests

mod proptest_invariants {
    use nyne::types::SymbolLineRange;
    use proptest::prelude::*;

    use super::*;

    /// Strategy: generate valid Rust source with 1-4 functions, each optionally
    /// decorated with doc comments and/or attributes.
    fn rust_functions_strategy() -> impl Strategy<Value = String> {
        proptest::collection::vec(
            (
                // optional doc comment
                proptest::bool::ANY,
                // optional #[derive(Debug)] attribute
                proptest::bool::ANY,
                // function name suffix (ensures unique names)
                0u32..100,
                // body: number of statements
                0usize..3,
            ),
            1..=4,
        )
        .prop_map(|funcs| {
            let mut source = String::new();
            for (i, (has_doc, has_attr, suffix, body_lines)) in funcs.into_iter().enumerate() {
                if i > 0 {
                    source.push('\n');
                }
                if has_doc {
                    source.push_str(&format!("/// Doc for func_{suffix}.\n"));
                }
                if has_attr {
                    source.push_str("#[inline]\n");
                }
                source.push_str(&format!("fn func_{suffix}() {{\n"));
                for j in 0..body_lines {
                    source.push_str(&format!("    let _v{j} = {j};\n"));
                }
                source.push_str("}\n");
            }
            source
        })
    }

    /// Strategy: generate valid Python source with 1-4 functions/classes.
    fn python_functions_strategy() -> impl Strategy<Value = String> {
        proptest::collection::vec(
            (
                proptest::bool::ANY, // decorator
                proptest::bool::ANY, // docstring
                0u32..100,           // name suffix
            ),
            1..=4,
        )
        .prop_map(|funcs| {
            let mut source = String::new();
            for (i, (has_decorator, has_docstring, suffix)) in funcs.into_iter().enumerate() {
                if i > 0 {
                    source.push('\n');
                }
                if has_decorator {
                    source.push_str("@staticmethod\n");
                }
                source.push_str(&format!("def func_{suffix}():\n"));
                if has_docstring {
                    source.push_str(&format!("    \"\"\"Doc for func_{suffix}.\"\"\"\n"));
                }
                source.push_str("    pass\n");
            }
            source
        })
    }

    /// Strategy: generate valid TypeScript source with 1-4 functions.
    fn typescript_functions_strategy() -> impl Strategy<Value = String> {
        proptest::collection::vec(
            (
                proptest::bool::ANY, // JSDoc
                0u32..100,           // name suffix
            ),
            1..=4,
        )
        .prop_map(|funcs| {
            let mut source = String::new();
            for (i, (has_jsdoc, suffix)) in funcs.into_iter().enumerate() {
                if i > 0 {
                    source.push('\n');
                }
                if has_jsdoc {
                    source.push_str(&format!("/** Doc for func_{suffix}. */\n"));
                }
                source.push_str(&format!("function func_{suffix}() {{}}\n"));
            }
            source
        })
    }

    /// Helper: recursively check invariants on a list of fragments.
    fn check_fragment_invariants(source: &str, fragments: &[Fragment]) {
        // 1. Sibling full_span ranges must not overlap (for non-structural children).
        //    Structural children (Docstring, Decorator) may not be in source order
        //    relative to each other (e.g. Python docstrings are inside the body,
        //    decorators before it).
        let non_structural: Vec<_> = fragments
            .iter()
            .filter(|f| !matches!(f.kind, FragmentKind::Docstring | FragmentKind::Decorator))
            .collect();
        for pair in non_structural.windows(2) {
            assert!(
                pair[0].full_span().end <= pair[1].full_span().start,
                "sibling full_spans overlap: {:?} ({}) vs {:?} ({})",
                pair[0].full_span(),
                pair[0].name,
                pair[1].full_span(),
                pair[1].name,
            );
        }

        for frag in fragments {
            // 2. full_span contains byte_range.
            assert!(
                frag.full_span().start <= frag.byte_range.start && frag.byte_range.end <= frag.full_span().end,
                "full_span {:?} does not contain byte_range {:?} for {}",
                frag.full_span(),
                frag.byte_range,
                frag.name,
            );

            // 3. Byte ranges are within source bounds.
            assert!(
                frag.full_span().end <= source.len(),
                "full_span.end {} exceeds source len {} for {}",
                frag.full_span().end,
                source.len(),
                frag.name,
            );

            // 4. line_range ↔ byte_range consistency.
            let from_line = SymbolLineRange::from_zero_based(&frag.line_range(source));
            let from_bytes = SymbolLineRange::from_byte_range(source, &frag.full_span());
            assert_eq!(
                from_line, from_bytes,
                "line_range mismatch for {}: from_zero_based={from_line:?}, from_byte_range={from_bytes:?}",
                frag.name,
            );

            // 5. Recurse into children.
            check_fragment_invariants(source, &frag.children);
        }
    }

    proptest! {
        #[test]
        fn rust_decomposition_invariants(source in rust_functions_strategy()) {
            check_fragment_invariants(&source, &decompose("rs", &source));
        }

        #[test]
        fn python_decomposition_invariants(source in python_functions_strategy()) {
            check_fragment_invariants(&source, &decompose("py", &source));
        }

        #[test]
        fn typescript_decomposition_invariants(source in typescript_functions_strategy()) {
            check_fragment_invariants(&source, &decompose("ts", &source));
        }
    }
}
