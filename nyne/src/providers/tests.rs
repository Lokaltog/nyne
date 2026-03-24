use super::*;

/// Tests that strip_companion_suffix extracts the base name correctly.
#[test]
fn strip_companion_suffix_basic() {
    assert_eq!(strip_companion_suffix("Foo@"), Some("Foo"));
    assert_eq!(strip_companion_suffix("file.rs@"), Some("file.rs"));
}

/// Tests that strip_companion_suffix rejects bare suffix and missing suffix.
#[test]
fn strip_companion_suffix_rejects_bare_and_missing() {
    assert_eq!(strip_companion_suffix("@"), None);
    assert_eq!(strip_companion_suffix("Foo"), None);
    assert_eq!(strip_companion_suffix(""), None);
}
