use rstest::rstest;

use super::*;

/// Verifies that `SessionInfo` survives JSON serialization and deserialization.
#[rstest]
fn session_info_roundtrip() {
    let info = SessionInfo {
        id: SessionId::new("test-session".into()).unwrap(),
        pid: 12345,
        mount_path: PathBuf::from("/tmp/test"),
    };
    let json = serde_json::to_string(&info).unwrap();
    let parsed: SessionInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id.as_str(), "test-session");
    assert_eq!(parsed.pid, 12345);
    assert_eq!(parsed.mount_path, PathBuf::from("/tmp/test"));
}

/// Verifies that resolve fails when the registry has no sessions.
#[rstest]
fn registry_resolve_empty() {
    let registry = SessionRegistry { sessions: vec![] };
    assert!(registry.resolve(None).is_err());
    assert!(registry.resolve(Some("nonexistent")).is_err());
}

/// Verifies that resolve returns the only active session without requiring an explicit ID.
#[rstest]
fn registry_resolve_single_session() {
    let registry = SessionRegistry {
        sessions: vec![SessionInfo {
            id: SessionId::new("foo".into()).unwrap(),
            pid: 1,
            mount_path: PathBuf::from("/tmp/foo"),
        }],
    };
    // No ID → returns the only session.
    let info = registry.resolve(None).unwrap();
    assert_eq!(info.id.as_str(), "foo");
    // Explicit ID → returns matching session.
    let info = registry.resolve(Some("foo")).unwrap();
    assert_eq!(info.id.as_str(), "foo");
    // Wrong ID → error.
    assert!(registry.resolve(Some("bar")).is_err());
}

/// Verifies that resolve requires an explicit ID when multiple sessions are active.
#[rstest]
fn registry_resolve_multiple_sessions_requires_id() {
    let registry = SessionRegistry {
        sessions: vec![
            SessionInfo {
                id: SessionId::new("a".into()).unwrap(),
                pid: 1,
                mount_path: PathBuf::from("/tmp/a"),
            },
            SessionInfo {
                id: SessionId::new("b".into()).unwrap(),
                pid: 2,
                mount_path: PathBuf::from("/tmp/b"),
            },
        ],
    };
    // No ID with multiple → error.
    assert!(registry.resolve(None).is_err());
    // Explicit ID → works.
    assert_eq!(registry.resolve(Some("b")).unwrap().id.as_str(), "b");
}

/// Verifies that `is_active` returns true for known sessions and false for unknown ones.
#[rstest]
fn registry_is_active() {
    let registry = SessionRegistry {
        sessions: vec![SessionInfo {
            id: SessionId::new("active".into()).unwrap(),
            pid: 1,
            mount_path: PathBuf::from("/tmp/x"),
        }],
    };
    assert!(registry.is_active("active"));
    assert!(!registry.is_active("inactive"));
}
