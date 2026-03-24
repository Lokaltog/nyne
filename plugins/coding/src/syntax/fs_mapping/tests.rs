use super::*;

/// Verifies that a name with a tilde-separated kind suffix is split correctly.
#[test]
fn split_disambiguator_with_kind() {
    assert_eq!(split_disambiguator("Foo~Struct"), ("Foo", Some("Struct")));
    assert_eq!(
        split_disambiguator("Display_for_Foo~Impl"),
        ("Display_for_Foo", Some("Impl"))
    );
}

/// Verifies that a name without a tilde has no kind disambiguator.
#[test]
fn split_disambiguator_without_kind() {
    assert_eq!(split_disambiguator("Foo"), ("Foo", None));
    assert_eq!(split_disambiguator("my_function"), ("my_function", None));
}

/// Verifies edge cases: trailing tilde, leading tilde, and multiple tildes.
#[test]
fn split_disambiguator_edge_cases() {
    // Trailing tilde — empty kind, treated as no disambiguator.
    assert_eq!(split_disambiguator("Foo~"), ("Foo~", None));
    // Leading tilde — empty base, treated as no disambiguator.
    assert_eq!(split_disambiguator("~Struct"), ("~Struct", None));
    // Multiple tildes — splits on the last one.
    assert_eq!(split_disambiguator("A~B~C"), ("A~B", Some("C")));
}
