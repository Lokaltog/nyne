use std::path::PathBuf;

use nyne::text::slugify_unbounded;
use rstest::{fixture, rstest};

use super::fragment::{DecomposedFile, Fragment, FragmentKind, SymbolKind};
use super::fragment_vfs_name;
use crate::test_support::registry;

/// Decompose source into fragments.
fn decompose(ext: &str, source: &str) -> DecomposedFile {
    let reg = registry();
    let d = reg.get(ext).unwrap();
    let (result, _tree) = d.decompose(source, 5);
    result
}

/// Decompose source and assign filesystem names.
fn decompose_mapped(ext: &str, source: &str) -> Vec<Fragment> {
    let d = registry().get(ext).unwrap().clone();
    let (mut result, _tree) = d.decompose(source, 5);
    d.assign_fs_names(&mut result);
    result
}

// Registry

/// Verifies that all expected language extensions are registered in the syntax registry.
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

/// Verifies that a single Rust function decomposes correctly via snapshot.
#[test]
fn rust_decompose_function() {
    insta::assert_debug_snapshot!(decompose("rs", "pub fn hello() {}\n"));
}

/// Verifies that a Rust struct with an impl block decomposes into separate fragments.
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

/// Verifies that Python functions and classes decompose into expected fragments.
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

/// Verifies that TypeScript functions and interfaces decompose into expected fragments.
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

/// Verifies that a Python decorated function decomposes correctly.
#[test]
fn python_decorated_function() {
    insta::assert_debug_snapshot!(decompose("py", "@app.route('/')\ndef index():\n    pass\n"));
}

/// Verifies that a Python module-level variable is extracted as a fragment.
#[test]
fn python_module_variable() {
    insta::assert_debug_snapshot!(decompose("py", "MAX_SIZE = 100\n"));
}

/// Verifies that a Python decorated class decomposes correctly.
#[test]
fn python_decorated_class() {
    insta::assert_debug_snapshot!(decompose("py", "@dataclass\nclass Point:\n    x: int\n    y: int\n"));
}

/// Verifies that a TypeScript const variable is extracted as a fragment.
#[test]
fn typescript_const_variable() {
    insta::assert_debug_snapshot!(decompose("ts", "export const API_URL = 'https://example.com';\n"));
}

/// Verifies that TSX files use a different tree-sitter grammar than TS.
#[test]
fn tsx_uses_different_grammar() {
    insta::assert_debug_snapshot!(decompose(
        "tsx",
        "export function App() {\n    return <div>Hello</div>;\n}\n"
    ));
}

/// Verifies that a TypeScript class with methods decomposes correctly.
#[test]
fn typescript_class_with_methods() {
    insta::assert_debug_snapshot!(decompose("ts", "class Greeter {\n    greet() { return 'hi'; }\n}\n"));
}

/// Verifies that a TypeScript enum declaration decomposes correctly.
#[test]
fn typescript_enum_declaration() {
    insta::assert_debug_snapshot!(decompose(
        "ts",
        "export enum Color {\n    Red,\n    Green,\n    Blue,\n}\n"
    ));
}

/// Verifies that markdown headings decompose into section fragments.
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

/// Verifies that markdown fragment names are slugified for filesystem mapping.
#[test]
fn markdown_fs_mapping_slugified() {
    let frags = decompose_mapped("md", "# Getting Started\n\nText.\n\n## Quick Setup\n\nMore.\n");
    insta::assert_debug_snapshot!(frags);
}

/// Verifies that a trait impl is named as `TraitName_for_TypeName`.
#[test]
fn rust_trait_impl_naming() {
    let source =
        "impl Display for Foo {\n    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { Ok(()) }\n}\n";
    insta::assert_debug_snapshot!(decompose("rs", source));
}

/// Verifies that Rust use statements are extracted into an Imports fragment.
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
/// `SpliceMode::Line` languages (including markdown). Using `line_end_of(full_span.end)`
/// would extend past the section boundary into the next heading — causing
/// duplicate headers on round-trip writes.
#[test]
fn markdown_section_body_roundtrip_does_not_duplicate_next_heading() {
    use crate::test_support::{line_start_of, splice_content};

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
    use crate::test_support::{line_start_of, splice_validate_write};

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

        let fs = nyne::router::fs::os::OsFilesystem::new(dir.path());
        let vfs_path = PathBuf::from("test.md");

        let body_start = line_start_of(fixture, section.full_span().start);
        let body_end = section.full_span().end;
        let read_body = &fixture[body_start..body_end];

        splice_validate_write(&fs, &vfs_path, body_start..body_end, read_body, |spliced| {
            let (re_decomposed, _tree) = decomposer.decompose(spliced, 5);
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
    use crate::test_support::{line_start_of, splice_content};

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

/// Verifies that Rust doc comments are extracted and cleaned to plain text.
#[test]
fn rust_doc_comment_extracted() {
    let result = decompose("rs", "/// Does a thing.\npub fn thing() {}\n");
    assert_eq!(result.len(), 1);
    let doc = registry().get("rs").unwrap().clean_doc_comment("/// Does a thing.");
    assert_eq!(doc.as_deref(), Some("Does a thing."));
}

/// Verifies that a doc comment extends the `full_span` to include lines before the fn keyword.
#[test]
fn rust_doc_comment_range_extends_fragment() {
    let source = "/// Doc line.\npub fn documented() {}\n";
    let result = decompose("rs", source);
    let frag = &result[0];
    let rope = crop::Rope::from(source);

    assert_eq!(frag.span.byte_range.start, 14, "byte_range should start at fn keyword");
    assert_eq!(frag.full_span().start, 0, "full_span should include doc comment");
    assert_eq!(frag.line_range(&rope).start, 0, "line_range should include doc comment");
}

/// Verifies that a Python class body-internal docstring does not shrink the byte range.
#[test]
fn python_body_internal_docstring_does_not_shrink_range() {
    let result = decompose("py", "class Foo:\n    \"\"\"Docstring.\"\"\"\n    x = 1\n");
    let frag = &result[0];

    assert_eq!(
        frag.span.byte_range.start, 0,
        "byte_range should start at class keyword"
    );
    assert_eq!(frag.full_span().start, 0, "full_span should start at class keyword");
    assert_eq!(
        frag.full_span().end,
        frag.span.byte_range.end,
        "full_span should match byte_range for body-internal docstrings"
    );
}

// Group 1: No doc comment returns None

/// Verifies that symbols without doc comments have no Docstring child fragment.
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

/// Verifies that symbols without decorators have no Decorator child fragment.
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

/// Verifies that a doc comment byte range covers expected text and excludes the symbol body.
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
    let text = &source[doc_child.span.byte_range.clone()];
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

/// Verifies that doc comments are captured as Docstring child fragments.
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

/// Verifies that a decorator byte range covers expected text and excludes the symbol body.
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
    let text = &source[dec_child.span.byte_range.clone()];
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

/// Verifies that file-level doc comments are extracted as standalone Docstring fragments.
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
    let raw = &source[doc_frag.span.byte_range.clone()];
    let cleaned = d.clean_doc_comment(raw).expect("should clean doc comment");
    assert_eq!(cleaned, expected);
}

/// Verifies that stripping and re-wrapping a file-level doc comment produces the original text.
#[rstest]
#[case::rust("rs", "//! Module-level doc.\n//! Second line.\n\nfn foo() {}\n")]
#[case::python("py", "\"\"\"Module docstring.\nSecond line.\n\"\"\"\n\ndef foo():\n    pass\n")]
fn file_level_doc_strip_wrap_roundtrip(#[case] ext: &str, #[case] source: &str) {
    let result = decompose(ext, source);
    let reg = registry();
    let d = reg.get(ext).unwrap();
    let doc_frag = super::fragment::find_fragment_of_kind(&result, &FragmentKind::Docstring)
        .expect("should have file-level docstring fragment");
    let raw = &source[doc_frag.span.byte_range.clone()];
    let stripped = d.strip_doc_comment(raw);
    let rewrapped = d.wrap_file_doc_comment(&stripped, "");
    assert_eq!(rewrapped, raw, "strip → wrap_file_doc_comment should round-trip");
}

// Group 7: Disjoint doc+decorator ranges

/// Verifies that docstring and decorator byte ranges never overlap on the same symbol.
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
        doc.span.byte_range.end <= dec.span.byte_range.start || dec.span.byte_range.end <= doc.span.byte_range.start,
        "ranges should not overlap"
    );
}

// Group 8: Line range symlink tests (byte_and_line_ranges_match)

/// Verifies that `line_range` and `byte_range` produce consistent `SymbolLineRange` values.
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
    use nyne::SymbolLineRange;

    let result = decompose(ext, source);

    fn check_fragments(rope: &crop::Rope, source: &str, fragments: &[Fragment]) {
        for frag in fragments {
            let from_line = SymbolLineRange::from_zero_based(&frag.line_range(rope));
            let from_bytes = SymbolLineRange::from_byte_range(source, &frag.full_span());
            assert_eq!(
                from_line,
                from_bytes,
                "line_range mismatch for '{}': from_zero_based={from_line:?}, from_byte_range={from_bytes:?}, \
                 full_span={:?}, line_range={:?}",
                frag.name,
                frag.full_span(),
                frag.line_range(rope),
            );
            check_fragments(rope, source, &frag.children);
        }
    }
    let rope = crop::Rope::from(source);
    check_fragments(&rope, source, &result);
}

// Line range: individual tests (from former line_range_symlink_tests submod)

/// Verifies that a decorator byte range converts to the correct line range.
#[test]
fn decorator_byte_range_to_line_range() {
    use nyne::SymbolLineRange;

    let source = "/// Doc.\n#[derive(Debug)]\npub struct Foo;\n";
    let result = decompose("rs", source);
    let dec_child = result[0]
        .child_of_kind(&FragmentKind::Decorator)
        .expect("should have decorator child");
    let line_range = SymbolLineRange::from_byte_range(source, &dec_child.span.byte_range);
    assert_eq!(line_range, SymbolLineRange { start: 2, end: 2 });
}

/// Verifies that an import span line range matches its byte range conversion.
#[test]
fn import_span_line_range_matches_byte_range() {
    use nyne::SymbolLineRange;

    let source = "use std::io;\nuse std::fmt;\n\nfn foo() {}\n";
    let result = decompose("rs", source);
    let imports = super::fragment::find_fragment_of_kind(&result, &FragmentKind::Imports).expect("should have imports");
    let rope = crop::Rope::from(source);
    let from_line = SymbolLineRange::from_zero_based(&imports.line_range(&rope));
    let from_bytes = SymbolLineRange::from_byte_range(source, &imports.span.byte_range);
    assert_eq!(
        from_line, from_bytes,
        "import line_range mismatch: from_zero_based={from_line:?}, from_byte_range={from_bytes:?}"
    );
}

/// Verifies that `SymbolLineRange` Display round-trips through `parse_slice_suffix`.
#[test]
fn lines_suffix_roundtrips_through_parse() {
    use nyne::{SymbolLineRange, parse_slice_suffix};

    let range = SymbolLineRange { start: 5, end: 10 };
    let suffix = range.to_string();
    let (base, spec) = parse_slice_suffix(&suffix).expect("should parse");
    assert_eq!(base, "lines");
    assert_eq!(spec, nyne::SliceSpec::Range(5, 10));

    let single = SymbolLineRange { start: 3, end: 3 };
    let suffix = single.to_string();
    let (base, spec) = parse_slice_suffix(&suffix).expect("should parse");
    assert_eq!(base, "lines");
    assert_eq!(spec, nyne::SliceSpec::Single(3));
}

// full_span tests (unique assertions per test)

/// How a fragment's `full_span` relates to its `byte_range`, depending on
/// whether the language includes decorators inside the wrapper node (Python)
/// or as a prefix preceding it (Rust, TypeScript).
#[derive(Copy, Clone, Debug)]
enum DecoratorMode {
    /// No decorator or doc — `full_span` and `byte_range` are identical.
    Bare,
    /// Decorator is inside the wrapper node (Python): both ranges start at 0.
    Wrapped,
    /// Decorator/doc is a prefix before the wrapper node (Rust, TypeScript,
    /// Rust with doc comment): `full_span.start < byte_range.start`.
    Prefixed,
}

/// Verifies `full_span` vs `byte_range` semantics for every (language, decorator)
/// combination.
#[rstest]
#[case::bare_rust("rs", "fn bare() {}\n", DecoratorMode::Bare)]
#[case::rust_decorator_only("rs", "#[derive(Debug)]\npub struct Foo;\n", DecoratorMode::Prefixed)]
#[case::rust_decorator_and_doc(
    "rs",
    "/// Documented.\n#[derive(Debug)]\npub struct Foo;\n",
    DecoratorMode::Prefixed
)]
#[case::python_decorated_class_with_docstring(
    "py",
    "@dataclass\nclass Bar:\n    \"\"\"A bar.\"\"\"\n    x: int = 0\n",
    DecoratorMode::Wrapped
)]
#[case::typescript_jsdoc("ts", "/** Documented. */\nfunction greet() {}\n", DecoratorMode::Prefixed)]
fn full_span_vs_byte_range(#[case] ext: &str, #[case] source: &str, #[case] mode: DecoratorMode) {
    let result = decompose(ext, source);
    let frag = &result[0];
    let full = frag.full_span();
    let byte = &frag.span.byte_range;

    match mode {
        DecoratorMode::Bare => {
            assert_eq!(full, *byte, "bare symbol: full_span == byte_range");
        }
        DecoratorMode::Wrapped => {
            assert_eq!(full.start, 0, "wrapper node should start at 0");
            assert_eq!(byte.start, 0, "wrapper node includes decorator");
            assert_eq!(full.end, byte.end, "ends match");
        }
        DecoratorMode::Prefixed => {
            assert_eq!(full.start, 0, "full_span should include leading doc/decorator");
            assert!(byte.start > 0, "byte_range should start after leading doc/decorator");
            assert_eq!(full.end, byte.end, "ends match");
        }
    }
}

/// Verifies that `full_span` ranges of adjacent Python classes do not overlap.
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
        first.span.byte_range.start, 0,
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

#[rstest]
#[case::rust("rs", "/// Hello\n/// World", "Hello\nWorld")]
#[case::python("py", "\"\"\"Hello world.\"\"\"", "Hello world.")]
#[case::typescript("ts", "/**\n * Hello\n * World\n */", "Hello\nWorld")]
fn strip_doc_comment(#[case] ext: &str, #[case] input: &str, #[case] expected: &str) {
    let stripped = registry().get(ext).unwrap().strip_doc_comment(input);
    assert_eq!(stripped, expected);
}

/// Verifies that `wrap_doc_comment` round-trips with strip for Rust doc comments.
#[test]
fn rust_wrap_doc_comment_roundtrip() {
    let reg = registry();
    let d = reg.get("rs").unwrap();
    let wrapped = d.wrap_doc_comment("Hello\nWorld", "    ");
    assert!(wrapped.starts_with("/// Hello"));
    assert!(wrapped.contains("    /// World"));
}

// Visibility

#[rstest]
#[case::rust_pub_crate("rs", "pub(crate) fn internal() {}\n", Some("pub(crate)"))]
#[case::typescript_export("ts", "export function greet() {}\n", Some("export"))]
fn visibility_extracted(#[case] ext: &str, #[case] source: &str, #[case] expected: Option<&str>) {
    let result = decompose(ext, source);
    assert_eq!(result[0].visibility.as_deref(), expected);
}

// Docstring splice round-trip

/// Verifies that a Rust docstring can be stripped, modified, re-wrapped, and spliced back.
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
    let doc_range = &doc_child.span.byte_range;
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

/// Tests that TypeScript decorator extraction on non-exported classes is not yet supported.
#[test]
#[ignore = "TS decorator extraction only works on exported classes — needs unwrap_wrapper support"]
fn typescript_decorator_on_class() {
    let source = "@Injectable()\nclass Service {}\n";
    let result = decompose("ts", source);
    let dec_child = result[0]
        .child_of_kind(&FragmentKind::Decorator)
        .expect("TS decorator should be captured");
    let text = &source[dec_child.span.byte_range.clone()];
    assert!(
        text.contains("@Injectable"),
        "range should cover the decorator: got {text:?}"
    );
}

// FS mapping / conflict resolution

/// Verifies that `fs_name` mapping preserves function names as-is for Rust.
#[test]
fn identity_fs_mapping() {
    let frags = decompose_mapped("rs", "fn alpha() {}\nfn beta() {}\n");
    assert_eq!(frags[0].fs_name.as_deref(), Some("alpha"));
    assert_eq!(frags[1].fs_name.as_deref(), Some("beta"));
}

/// Verifies that structural fragments (imports, docstrings, decorators) have no `fs_name`.
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

/// Verifies that same-name symbols of different kinds get kind-suffix disambiguation.
#[test]
fn kind_suffix_conflict_resolution() {
    use super::fragment::{ConflictEntry, ConflictSet};
    use super::fs_mapping::{ConflictStrategy, resolve_conflicts};

    let result = decompose_mapped("rs", "struct Foo;\nfn Foo() {}\n");
    let resolutions = resolve_conflicts(
        &[ConflictSet {
            name: "Foo".to_owned(),
            entries: vec![
                ConflictEntry {
                    index: 0,
                    fragment_kind: result[0].kind.clone(),
                },
                ConflictEntry {
                    index: 1,
                    fragment_kind: result[1].kind.clone(),
                },
            ],
        }],
        ConflictStrategy::KindSuffix,
    );
    assert_eq!(resolutions.len(), 2);
    assert_eq!(resolutions[0].fs_name.as_deref(), Some("Foo~Struct"));
    assert_eq!(resolutions[1].fs_name.as_deref(), Some("Foo~Function"));
}

/// Verifies that same-name same-kind symbols get numbered disambiguation.
#[test]
fn numbered_conflict_resolution() {
    use super::fragment::{ConflictEntry, ConflictSet};
    use super::fs_mapping::{ConflictStrategy, resolve_conflicts};

    let result = decompose_mapped("md", "# Intro\n\n# Intro\n\n# Intro\n");
    let resolutions = resolve_conflicts(
        &[ConflictSet {
            name: result[0].fs_name.clone().unwrap(),
            entries: result
                .iter()
                .enumerate()
                .map(|(i, f)| ConflictEntry {
                    index: i,
                    fragment_kind: f.kind.clone(),
                })
                .collect(),
        }],
        ConflictStrategy::Numbered,
    );
    assert_eq!(resolutions[0].fs_name.as_deref(), Some("00-intro"));
    assert_eq!(resolutions[1].fs_name.as_deref(), Some("00-intro-2"));
    assert_eq!(resolutions[2].fs_name.as_deref(), Some("00-intro-3"));
}

/// Verifies that slugify converts headings to lowercase-hyphenated form.
#[test]
fn slugify_basic() {
    assert_eq!(slugify_unbounded("Getting Started"), "getting-started");
    assert_eq!(slugify_unbounded("API Reference"), "api-reference");
    assert_eq!(slugify_unbounded("  leading & trailing  "), "leading-trailing");
    assert_eq!(slugify_unbounded("CamelCase"), "camel-case");
}

// Validation

#[rstest]
#[case::valid("fn foo() {}\n", true)]
#[case::invalid("fn foo( {}\n", false)]
fn validate_source(#[case] source: &str, #[case] should_pass: bool) {
    assert_eq!(registry().get("rs").unwrap().validate(source).is_ok(), should_pass);
}

#[rstest]
#[case::heading_with_newline("# Heading\n", true)]
#[case::heading_without_newline("# Heading", false)]
#[case::multi_section("# A\n\nText.\n\n## B\n\nMore.\n", true)]
fn validate_markdown(#[case] source: &str, #[case] should_pass: bool) {
    let reg = registry();
    assert_eq!(reg.get("md").unwrap().validate(source).is_ok(), should_pass);
}

// extract_symbol

#[rstest]
#[case::top_level("fn hello() {}\n\nfn world() { 42 }\n", "rs", &["world"], Some("fn world() { 42 }"))]
#[case::nested("impl Foo {\n    fn bar() {}\n    fn baz() {}\n}\n", "rs", &["Foo", "baz"], Some("fn baz() {}"))]
#[case::missing("fn hello() {}\n", "rs", &["nonexistent"], None)]
#[case::unknown_ext("some content", "xyz", &["foo"], None)]
fn extract_symbol(#[case] source: &str, #[case] ext: &str, #[case] path: &[&str], #[case] expected: Option<&str>) {
    let reg = registry();
    let path: Vec<String> = path.iter().map(|s| (*s).into()).collect();
    let result = reg.extract_symbol(source, ext, &path, 5);
    assert_eq!(result.as_deref().map(str::trim), expected);
}

// Registry compound lookup

/// Verifies that compound registry lookup returns a decomposer for known inner+outer pairs.
#[test]
fn registry_compound_lookup_returns_decomposer() {
    let reg = registry();
    assert!(reg.get_compound("md", "j2").is_some(), "missing compound md+j2");
    assert!(reg.get_compound("toml", "j2").is_some(), "missing compound toml+j2");
    assert!(reg.get_compound("rs", "j2").is_some(), "missing compound rs+j2");
}

#[rstest]
#[case::unknown_inner("xyz", "j2")]
#[case::unknown_outer("rs", "gz")]
fn registry_compound_unknown_returns_none(#[case] inner: &str, #[case] outer: &str) {
    assert!(registry().get_compound(inner, outer).is_none());
}

/// Verifies that simple extension lookups still work when compound decomposers are registered.
#[test]
fn registry_simple_lookup_unaffected_by_compound() {
    let reg = registry();
    assert!(reg.get("rs").is_some());
    assert!(reg.get("py").is_some());
    assert!(reg.get("md").is_some());
}

/// Verifies that every registered extension has a corresponding j2 compound decomposer.
#[test]
fn registry_has_all_j2_compounds() {
    let reg = registry();
    for ext in reg.extensions() {
        assert!(reg.get_compound(ext, "j2").is_some(), "missing compound {ext}+j2");
    }
}

#[rstest]
#[case::simple_extension("src/main.rs", true)]
#[case::compound_j2("templates/page.md.j2", true)]
#[case::unknown_extension("data/file.xyz", false)]
fn decomposer_for_resolves(#[case] path: &str, #[case] expected: bool) {
    assert_eq!(registry().decomposer_for(&PathBuf::from(path)).is_some(), expected);
}

/// Verifies that compound j2 decomposers produce fragments from Jinja2 content.
#[rstest]
#[case::md_j2("templates/page.md.j2")]
#[case::toml_j2("config/app.toml.j2")]
#[case::py_j2("scripts/build.py.j2")]
fn decomposer_for_compound_decomposes_j2_content(#[case] path_str: &str) {
    let reg = registry();
    let path = PathBuf::from(path_str);
    let decomposer = reg.decomposer_for(&path).expect("compound decomposer should exist");

    let source = "{% block content %}\nHello\n{% endblock %}";
    let (result, _tree) = decomposer.decompose(source, 5);
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
/// Two inherent impl blocks trigger `KindSuffix` conflict: `Foo~Impl` appears
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

/// Verifies that `find_nearest_fragment_at_line` returns the closest fragment for a given line.
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
    assert_eq!(
        super::find_nearest_fragment_at_line(&frags, line, &crop::Rope::from(source.as_str())).as_deref(),
        Some(expected.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>().as_slice())
    );
}

/// Verifies that `find_nearest_fragment_at_line` returns None for empty fragment lists.
#[test]
fn nearest_fragment_at_line_empty_fragments_returns_none() {
    assert_eq!(
        super::find_nearest_fragment_at_line(&[], 5, &crop::Rope::from("")),
        None
    );
}

/// Verifies that `find_fragment_at_line` resolves a child inside a nameless impl parent.
#[rstest]
fn fragment_at_line_inside_nameless_parent(nameless_impl_fragments: (Vec<Fragment>, String)) {
    let (frags, source) = nameless_impl_fragments;
    // Line 3 is inside bar, child of a nameless impl — should find bar.
    assert_eq!(
        super::find_fragment_at_line(&frags, 3, &crop::Rope::from(source.as_str())).as_deref(),
        Some(&["bar".to_owned()][..])
    );
}

/// Verifies that `find_nearest_fragment_at_line` resolves children of nameless impl parents.
#[rstest]
#[case::exact_child(3, &["bar"])]
#[case::gap_in_nameless_parent(2, &["bar"])]
fn nearest_fragment_at_line_nameless_parent(
    nameless_impl_fragments: (Vec<Fragment>, String),
    #[case] line: usize,
    #[case] expected: &[&str],
) {
    let (frags, source) = nameless_impl_fragments;
    assert_eq!(
        super::find_nearest_fragment_at_line(&frags, line, &crop::Rope::from(source.as_str())).as_deref(),
        Some(expected.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>().as_slice())
    );
}

// Property-based tests

/// Property-based tests verifying decomposition invariants across generated source code.
mod proptest_invariants {
    use nyne::SymbolLineRange;
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
        let rope = crop::Rope::from(source);
        check_fragment_invariants_rope(source, &rope, fragments);
    }

    fn check_fragment_invariants_rope(source: &str, rope: &crop::Rope, fragments: &[Fragment]) {
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
                frag.full_span().start <= frag.span.byte_range.start
                    && frag.span.byte_range.end <= frag.full_span().end,
                "full_span {:?} does not contain byte_range {:?} for {}",
                frag.full_span(),
                frag.span.byte_range,
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
            let from_line = SymbolLineRange::from_zero_based(&frag.line_range(rope));
            let from_bytes = SymbolLineRange::from_byte_range(source, &frag.full_span());
            assert_eq!(
                from_line, from_bytes,
                "line_range mismatch for {}: from_zero_based={from_line:?}, from_byte_range={from_bytes:?}",
                frag.name,
            );

            // 5. Recurse into children.
            check_fragment_invariants_rope(source, rope, &frag.children);
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
    #[rstest]
    #[case::single(&["Foo"], "Foo")]
    #[case::nested(&["Foo", "bar"], "Foo@/bar")]
    #[case::deep(&["Impl", "method", "inner"], "Impl@/method@/inner")]
    fn fragment_vfs_name_joins_with_companion_separator(#[case] segments: &[&str], #[case] expected: &str) {
        let companion = nyne_companion::Companion::new(None, "@".into());
        assert_eq!(fragment_vfs_name(&companion, segments), expected);
    }
}
