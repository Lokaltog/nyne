use rstest::rstest;

use super::*;

fn session(id: &str, pid: i32, path: &str) -> SessionInfo {
    SessionInfo {
        id: SessionId::new(id.into()).unwrap(),
        pid,
        mount_path: PathBuf::from(path),
    }
}

fn registry(entries: &[(&str, i32, &str)]) -> SessionRegistry {
    SessionRegistry {
        sessions: entries.iter().map(|(id, pid, path)| session(id, *pid, path)).collect(),
    }
}
/// Verifies that `SessionInfo` survives JSON serialization and deserialization.
#[rstest]
fn session_info_roundtrip() {
    let info = session("test-session", 12345, "/tmp/test");
    let json = serde_json::to_string(&info).unwrap();
    let parsed: SessionInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id.as_str(), "test-session");
    assert_eq!(parsed.pid, 12345);
    assert_eq!(parsed.mount_path, PathBuf::from("/tmp/test"));
}

/// Verifies that `SessionRegistry::resolve` handles all session count + ID combinations.
#[rstest]
// Empty registry: any resolve fails.
#[case::empty_no_id(&[], None, None)]
#[case::empty_with_id(&[], Some("nonexistent"), None)]
// Single session: no ID returns it; explicit matching ID returns it; wrong ID errors.
#[case::single_no_id(&[("foo", 1, "/tmp/foo")], None, Some("foo"))]
#[case::single_matching_id(&[("foo", 1, "/tmp/foo")], Some("foo"), Some("foo"))]
#[case::single_wrong_id(&[("foo", 1, "/tmp/foo")], Some("bar"), None)]
// Multiple sessions: no ID errors; explicit ID selects.
#[case::multi_no_id(&[("a", 1, "/tmp/a"), ("b", 2, "/tmp/b")], None, None)]
#[case::multi_with_id(&[("a", 1, "/tmp/a"), ("b", 2, "/tmp/b")], Some("b"), Some("b"))]
fn registry_resolve(
    #[case] entries: &[(&str, i32, &str)],
    #[case] query: Option<&str>,
    #[case] expected_id: Option<&str>,
) {
    let registry = registry(entries);
    match expected_id {
        Some(id) => assert_eq!(registry.resolve(query).unwrap().id.as_str(), id),
        None => assert!(registry.resolve(query).is_err()),
    }
}

/// Verifies that `is_active` returns true for known sessions and false for unknown ones.
#[rstest]
#[case::known("active", true)]
#[case::unknown("inactive", false)]
fn registry_is_active(#[case] query: &str, #[case] expected: bool) {
    let registry = registry(&[("active", 1, "/tmp/x")]);
    assert_eq!(registry.is_active(query), expected);
}
