use super::*;

#[test]
fn current_process_is_alive() {
    assert!(state::is_pid_alive(std::process::id() as i32));
}

#[test]
fn bogus_pid_is_not_alive() {
    assert!(!state::is_pid_alive(i32::MAX));
}

#[test]
fn session_info_roundtrip() {
    let info = SessionInfo {
        id: "test-session".into(),
        pid: 12345,
        mount_path: PathBuf::from("/tmp/test"),
    };
    let json = serde_json::to_string(&info).unwrap();
    let parsed: SessionInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, "test-session");
    assert_eq!(parsed.pid, 12345);
    assert_eq!(parsed.mount_path, PathBuf::from("/tmp/test"));
}

#[test]
fn registry_resolve_empty() {
    let registry = SessionRegistry { sessions: vec![] };
    assert!(registry.resolve(None).is_err());
    assert!(registry.resolve(Some("nonexistent")).is_err());
}

#[test]
fn registry_resolve_single_session() {
    let registry = SessionRegistry {
        sessions: vec![SessionInfo {
            id: "foo".into(),
            pid: 1,
            mount_path: PathBuf::from("/tmp/foo"),
        }],
    };
    // No ID → returns the only session.
    let info = registry.resolve(None).unwrap();
    assert_eq!(info.id, "foo");
    // Explicit ID → returns matching session.
    let info = registry.resolve(Some("foo")).unwrap();
    assert_eq!(info.id, "foo");
    // Wrong ID → error.
    assert!(registry.resolve(Some("bar")).is_err());
}

#[test]
fn registry_resolve_multiple_sessions_requires_id() {
    let registry = SessionRegistry {
        sessions: vec![
            SessionInfo {
                id: "a".into(),
                pid: 1,
                mount_path: PathBuf::from("/tmp/a"),
            },
            SessionInfo {
                id: "b".into(),
                pid: 2,
                mount_path: PathBuf::from("/tmp/b"),
            },
        ],
    };
    // No ID with multiple → error.
    assert!(registry.resolve(None).is_err());
    // Explicit ID → works.
    assert_eq!(registry.resolve(Some("b")).unwrap().id, "b");
}

#[test]
fn registry_is_active() {
    let registry = SessionRegistry {
        sessions: vec![SessionInfo {
            id: "active".into(),
            pid: 1,
            mount_path: PathBuf::from("/tmp/x"),
        }],
    };
    assert!(registry.is_active("active"));
    assert!(!registry.is_active("inactive"));
}
