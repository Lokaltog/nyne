use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};

use super::DiagnosticStore;

/// Build a dummy diagnostic for testing.
fn dummy_diagnostic(message: &str, severity: DiagnosticSeverity) -> Diagnostic {
    Diagnostic {
        range: Range::new(Position::new(0, 0), Position::new(0, 1)),
        severity: Some(severity),
        message: message.to_owned(),
        ..Default::default()
    }
}

// SC-1: Clean reads are non-blocking.
#[test]
fn clean_file_returns_immediately() {
    let store = DiagnosticStore::new();
    let path = Path::new("/src/main.rs");
    let diags = vec![dummy_diagnostic("unused var", DiagnosticSeverity::WARNING)];

    store.publish(path, diags);

    let start = Instant::now();
    let result = store.get_or_wait(path, Duration::from_secs(2));
    assert!(start.elapsed() < Duration::from_millis(10));
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].message, "unused var");
}

// SC-1 supplement: Unknown file returns empty immediately.
#[test]
fn unknown_file_returns_empty_immediately() {
    let store = DiagnosticStore::new();
    let start = Instant::now();
    let result = store.get_or_wait(Path::new("/no/such/file.rs"), Duration::from_secs(2));
    assert!(start.elapsed() < Duration::from_millis(10));
    assert!(result.is_empty());
}

// SC-2: Dirty file blocks until publish.
#[test]
fn dirty_file_blocks_until_publish() {
    let store = Arc::new(DiagnosticStore::new());
    let path = Path::new("/src/lib.rs");

    store.mark_dirty(path);

    let store2 = Arc::clone(&store);
    let handle = std::thread::spawn(move || {
        let start = Instant::now();
        let result = store2.get_or_wait(Path::new("/src/lib.rs"), Duration::from_secs(2));
        (start.elapsed(), result)
    });

    // Let the waiter park on the condvar.
    std::thread::sleep(Duration::from_millis(50));

    // Publish — should wake the waiter.
    let diags = vec![dummy_diagnostic("error here", DiagnosticSeverity::ERROR)];
    store.publish(path, diags);

    let (waited, result) = handle.join().unwrap();
    // Should unblock well before the 2s timeout.
    assert!(waited < Duration::from_millis(200), "waited {waited:?}");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].message, "error here");
}

// SC-3: Dirty file times out.
#[test]
fn dirty_file_times_out() {
    let store = DiagnosticStore::new();
    let path = Path::new("/src/timeout.rs");

    // Publish some initial diagnostics, then mark dirty.
    store.publish(path, vec![dummy_diagnostic("stale", DiagnosticSeverity::HINT)]);
    store.mark_dirty(path);

    let start = Instant::now();
    let result = store.get_or_wait(path, Duration::from_millis(100));
    let waited = start.elapsed();

    // Should wait approximately the timeout.
    assert!(waited >= Duration::from_millis(80), "waited only {waited:?}");
    assert!(waited < Duration::from_millis(300), "waited too long {waited:?}");
    // Returns stale diagnostics on timeout.
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].message, "stale");
}

// SC-4: Multiple waiters all wake on publish.
#[test]
fn multiple_waiters_all_wake() {
    let store = Arc::new(DiagnosticStore::new());
    let path = Path::new("/src/multi.rs");
    store.mark_dirty(path);

    let mut handles = Vec::new();
    for _ in 0..4 {
        let store = Arc::clone(&store);
        handles.push(std::thread::spawn(move || {
            store.get_or_wait(Path::new("/src/multi.rs"), Duration::from_secs(2))
        }));
    }

    // Let all waiters park.
    std::thread::sleep(Duration::from_millis(50));

    let diags = vec![
        dummy_diagnostic("err1", DiagnosticSeverity::ERROR),
        dummy_diagnostic("warn1", DiagnosticSeverity::WARNING),
    ];
    store.publish(path, diags);

    for handle in handles {
        let result = handle.join().unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].message, "err1");
        assert_eq!(result[1].message, "warn1");
    }
}

// SC-5: Publish for file X doesn't wake waiter for file Y.
#[test]
fn publish_for_other_file_does_not_unblock() {
    let store = Arc::new(DiagnosticStore::new());
    let path_a = Path::new("/src/a.rs");
    let path_b = Path::new("/src/b.rs");

    store.mark_dirty(path_a);
    store.mark_dirty(path_b);

    // Waiter on file A with a short timeout.
    let store2 = Arc::clone(&store);
    let handle = std::thread::spawn(move || {
        let start = Instant::now();
        let result = store2.get_or_wait(Path::new("/src/a.rs"), Duration::from_millis(200));
        (start.elapsed(), result)
    });

    // Let the waiter park.
    std::thread::sleep(Duration::from_millis(30));

    // Publish for file B only — should not satisfy file A's waiter.
    store.publish(path_b, vec![dummy_diagnostic("b diag", DiagnosticSeverity::ERROR)]);

    let (waited, result) = handle.join().unwrap();
    // File A should have timed out (waited ~200ms), not unblocked early.
    assert!(waited >= Duration::from_millis(150), "unblocked too early: {waited:?}");
    assert!(result.is_empty());
}

#[test]
fn remove_clears_entry() {
    let store = DiagnosticStore::new();
    let path = Path::new("/src/gone.rs");

    store.publish(path, vec![dummy_diagnostic("hi", DiagnosticSeverity::HINT)]);
    assert_eq!(store.get_or_wait(path, Duration::ZERO).len(), 1);

    store.remove(path);
    assert!(store.get_or_wait(path, Duration::ZERO).is_empty());
}

#[test]
fn mark_dirty_then_publish_cycle() {
    let store = DiagnosticStore::new();
    let path = Path::new("/src/cycle.rs");

    // Initial publish.
    store.publish(path, vec![dummy_diagnostic("v1", DiagnosticSeverity::ERROR)]);
    assert_eq!(store.get_or_wait(path, Duration::ZERO)[0].message, "v1");

    // Dirty → publish with updated content.
    store.mark_dirty(path);
    store.publish(path, vec![dummy_diagnostic("v2", DiagnosticSeverity::WARNING)]);
    let result = store.get_or_wait(path, Duration::ZERO);
    assert_eq!(result[0].message, "v2");
}
