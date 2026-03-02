use super::*;

#[test]
fn strip_companion_suffix_basic() {
    assert_eq!(strip_companion_suffix("Foo@"), Some("Foo"));
    assert_eq!(strip_companion_suffix("file.rs@"), Some("file.rs"));
}

#[test]
fn strip_companion_suffix_rejects_bare_and_missing() {
    assert_eq!(strip_companion_suffix("@"), None);
    assert_eq!(strip_companion_suffix("Foo"), None);
    assert_eq!(strip_companion_suffix(""), None);
}
