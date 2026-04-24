//! Test helpers for nyne-source.

use crate::syntax::SyntaxRegistry;
use crate::syntax::fragment::DecomposedFile;

/// Build a `SyntaxRegistry` with all compiled-in languages.
///
/// Shorthand for [`SyntaxRegistry::build()`] so tests don't need to import
/// the registry type directly.
pub fn registry() -> SyntaxRegistry { SyntaxRegistry::build() }

/// Decompose `source` with the language registered for `ext` and return the
/// fragment tree. Panics if no language is registered for `ext`.
///
/// Shared helper used by per-language `basic()` fixtures.
pub fn decompose_fixture(ext: &str, source: &str) -> DecomposedFile {
    let (result, _tree) = registry().get(ext).unwrap().decompose(source, 5);
    result
}
/// Assert that the fragment named `name` (searched recursively through nested
/// children) has exactly the expected child names.
///
/// The recursive search covers hierarchical languages (Markdown sections) and
/// flat ones (Python, TypeScript, Nix) with a single helper.
pub fn assert_fragment_children(decomposed: &crate::syntax::fragment::DecomposedFile, name: &str, expected: &[&str]) {
    fn find<'a>(
        frags: &'a [crate::syntax::fragment::Fragment],
        name: &str,
    ) -> Option<&'a crate::syntax::fragment::Fragment> {
        frags.iter().find_map(|f| {
            if f.name == name {
                Some(f)
            } else {
                find(&f.children, name)
            }
        })
    }
    let frag = find(decomposed, name).unwrap_or_else(|| panic!("no fragment named {name:?}"));
    let child_names: Vec<_> = frag.children.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(child_names, expected, "children of {name:?}");
}

use std::ops::Range;
use std::path::Path;
use std::str::from_utf8;

use color_eyre::eyre::Result;
use crop::Rope;
use nyne::router::Filesystem;

use crate::edit::splice::{line_start_of_rope, splice_rope_validate_write};

/// Read a source file, splice new content at a byte range, validate, and write back.
///
/// Test-only convenience: builds the rope from the file contents and delegates
/// to [`splice_rope_validate_write`].
pub fn splice_validate_write(
    fs: &dyn Filesystem,
    source_file: &Path,
    byte_range: Range<usize>,
    new_content: &str,
    validate: impl Fn(&str) -> Result<(), String>,
) -> Result<usize> {
    let content = fs.read_file(source_file)?;
    let mut rope = Rope::from(from_utf8(&content)?);
    splice_rope_validate_write(fs, source_file, &mut rope, byte_range, new_content, validate)
}

/// Splice new content into source text at a byte range, returning the result.
///
/// Test-only helper used by decomposition round-trip tests.
#[must_use]
pub fn splice_content(source: &str, byte_range: Range<usize>, new_content: &str) -> String {
    let mut rope = Rope::from(source);
    rope.replace(byte_range, new_content);
    rope.to_string()
}

/// Byte offset of the start of the line containing `offset`.
///
/// Test-only convenience wrapper that builds a [`Rope`] internally.
#[must_use]
pub fn line_start_of(source: &str, offset: usize) -> usize { line_start_of_rope(&Rope::from(source), offset) }

/// Generate the standard language-decomposition test skeleton.
///
/// Emits `load_basic()`, the `basic` rstest fixture, and the three
/// canonical assertions (`fragment_count`, `fragment_names`,
/// `fragment_kinds`). Language-specific tests (class children,
/// import-coalescing quirks, etc.) stay in the calling module.
///
/// `imports_contain` is optional — when provided, generates an
/// `imports_extracted` test asserting the `Imports` fragment range
/// contains each given substring.
///
/// # Example
///
/// ```ignore
/// crate::language_tests! {
///     ext: "rs",
///     fixture_module: "syntax/languages/rust",
///     fixture_file: "basic.rs",
///     fragment_count: 9,
///     fragment_names: [
///         "imports", "MAX_SIZE", "process", "helper",
///         "Config", "Status", "Processor",
///         "Processor_for_Config", "Config",
///     ],
///     fragment_kinds: [
///         FragmentKind::Imports,
///         FragmentKind::Symbol(SymbolKind::Const),
///         // ...
///     ],
///     imports_contain: ["use std::collections::HashMap;", "use std::io;"],
/// }
/// ```
#[macro_export]
macro_rules! language_tests {
    (
        ext: $ext:literal,
        fixture_module: $module:literal,
        fixture_file: $fixture:literal,
        fragment_count: $count:expr,
        fragment_names: [ $($name:expr),* $(,)? ],
        fragment_kinds: [ $($kind:expr),* $(,)? ]
        $(, imports_contain: [ $($import:expr),* $(,)? ] )?
        $(,)?
    ) => {
        /// Load the shared fixture source for this language.
        fn load_basic() -> ::std::string::String { ::nyne::load_fixture!($module, $fixture) }

        /// Fixture: decompose the basic fixture into fragments.
        #[::rstest::fixture]
        fn basic() -> $crate::syntax::fragment::DecomposedFile {
            $crate::test_support::decompose_fixture($ext, &load_basic())
        }

        #[::rstest::rstest]
        fn fragment_count(basic: $crate::syntax::fragment::DecomposedFile) {
            assert_eq!(basic.len(), $count);
        }

        #[::rstest::rstest]
        fn fragment_names(basic: $crate::syntax::fragment::DecomposedFile) {
            let names: ::std::vec::Vec<&str> = basic.iter().map(|f| f.name.as_str()).collect();
            assert_eq!(names, [$($name),*]);
        }

        #[::rstest::rstest]
        fn fragment_kinds(basic: $crate::syntax::fragment::DecomposedFile) {
            let kinds: ::std::vec::Vec<$crate::syntax::fragment::FragmentKind> =
                basic.iter().map(|f| f.kind.clone()).collect();
            let expected: ::std::vec::Vec<$crate::syntax::fragment::FragmentKind> = ::std::vec![$($kind),*];
            assert_eq!(kinds, expected);
        }

        $(
            /// Imports are coalesced into a single Imports fragment containing every
            /// expected substring.
            #[::rstest::rstest]
            fn imports_extracted(basic: $crate::syntax::fragment::DecomposedFile) {
                let range = $crate::syntax::fragment::find_fragment_of_kind(
                    &basic,
                    &$crate::syntax::fragment::FragmentKind::Imports,
                )
                .expect("imports fragment should be present")
                .span
                .byte_range
                .clone();
                let source = load_basic();
                $(
                    assert!(
                        source[range.clone()].contains($import),
                        "imports fragment should contain {:?}",
                        $import,
                    );
                )*
            }
        )?
    };
}
