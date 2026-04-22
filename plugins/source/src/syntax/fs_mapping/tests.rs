use rstest::rstest;

use super::*;
use crate::syntax::fragment::{ConflictEntry, SymbolKind};

/// Verifies that a name with a tilde-separated kind suffix is split correctly.
#[rstest]
fn split_disambiguator_with_kind() {
    assert_eq!(split_disambiguator("Foo~Struct"), ("Foo", Some("Struct")));
    assert_eq!(
        split_disambiguator("Display_for_Foo~Impl"),
        ("Display_for_Foo", Some("Impl"))
    );
}

/// Verifies that a name without a tilde has no kind disambiguator.
#[rstest]
fn split_disambiguator_without_kind() {
    assert_eq!(split_disambiguator("Foo"), ("Foo", None));
    assert_eq!(split_disambiguator("my_function"), ("my_function", None));
}

/// Verifies edge cases: trailing tilde, leading tilde, and multiple tildes.
#[rstest]
fn split_disambiguator_edge_cases() {
    // Trailing tilde — empty kind, treated as no disambiguator.
    assert_eq!(split_disambiguator("Foo~"), ("Foo~", None));
    // Leading tilde — empty base, treated as no disambiguator.
    assert_eq!(split_disambiguator("~Struct"), ("~Struct", None));
    // Multiple tildes — splits on the last one.
    assert_eq!(split_disambiguator("A~B~C"), ("A~B", Some("C")));
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
