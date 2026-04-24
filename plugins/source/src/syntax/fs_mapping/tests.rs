use rstest::rstest;

use super::*;
use crate::syntax::fragment::{ConflictEntry, SymbolKind};

/// Verifies the tilde-split behavior of `split_disambiguator` across every kind of input.
#[rstest]
#[case::with_kind_struct("Foo~Struct", "Foo", Some("Struct"))]
#[case::with_kind_impl("Display_for_Foo~Impl", "Display_for_Foo", Some("Impl"))]
#[case::plain_type("Foo", "Foo", None)]
#[case::plain_fn("my_function", "my_function", None)]
// Trailing tilde — empty kind, treated as no disambiguator.
#[case::trailing_tilde("Foo~", "Foo~", None)]
// Leading tilde — empty base, treated as no disambiguator.
#[case::leading_tilde("~Struct", "~Struct", None)]
// Multiple tildes — splits on the last one.
#[case::multiple_tildes("A~B~C", "A~B", Some("C"))]
fn split_disambiguator_cases(#[case] input: &str, #[case] expected_base: &str, #[case] expected_kind: Option<&str>) {
    assert_eq!(split_disambiguator(input), (expected_base, expected_kind));
}

/// When two fragments share the same name and the same kind, `~Kind` alone
/// doesn't disambiguate. The fallback appends `-N` to subsequent duplicates.
#[rstest]
fn resolve_kind_suffix_duplicate_kinds() {
    let conflicts = vec![ConflictSet {
        name: "Foo".to_owned(),
        entries: vec![
            ConflictEntry {
                index: 0,
                fragment_kind: FragmentKind::Symbol(SymbolKind::Struct),
            },
            ConflictEntry {
                index: 1,
                fragment_kind: FragmentKind::Symbol(SymbolKind::Impl),
            },
            ConflictEntry {
                index: 2,
                fragment_kind: FragmentKind::Symbol(SymbolKind::Impl),
            },
        ],
    }];

    let resolutions = resolve_conflicts(&conflicts, ConflictStrategy::KindSuffix);

    assert_eq!(resolutions.len(), 3);
    assert_eq!(resolutions[0].fs_name.as_deref(), Some("Foo~Struct"));
    assert_eq!(resolutions[1].fs_name.as_deref(), Some("Foo~Impl"));
    assert_eq!(resolutions[2].fs_name.as_deref(), Some("Foo~Impl-2"));
}

/// When all entries have unique kinds, no numeric suffix is needed.
#[rstest]
fn resolve_kind_suffix_unique_kinds() {
    let conflicts = vec![ConflictSet {
        name: "Foo".to_owned(),
        entries: vec![
            ConflictEntry {
                index: 0,
                fragment_kind: FragmentKind::Symbol(SymbolKind::Struct),
            },
            ConflictEntry {
                index: 1,
                fragment_kind: FragmentKind::Symbol(SymbolKind::Impl),
            },
        ],
    }];

    let resolutions = resolve_conflicts(&conflicts, ConflictStrategy::KindSuffix);

    assert_eq!(resolutions.len(), 2);
    assert_eq!(resolutions[0].fs_name.as_deref(), Some("Foo~Struct"));
    assert_eq!(resolutions[1].fs_name.as_deref(), Some("Foo~Impl"));
}
