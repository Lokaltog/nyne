use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};
use rstest::rstest;

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
/// Spawn a waiter on `waiter_path`, let it park, then publish `diags` on
/// `publish_path`. Returns `(elapsed, result)` from the waiter thread.
fn park_waiter_then_publish(
    store: &Arc<DiagnosticStore>,
    waiter_path: &'static str,
    timeout: Duration,
    publish_path: &Path,
    diags: Vec<Diagnostic>,
) -> (Duration, Vec<Diagnostic>) {
    let store2 = Arc::clone(store);
    let handle = std::thread::spawn(move || {
        let start = Instant::now();
        let result = store2.get_or_wait(Path::new(waiter_path), timeout);
        (start.elapsed(), result)
    });
    std::thread::sleep(Duration::from_millis(50));
    store.publish(publish_path, diags);
    handle.join().unwrap()
}

// SC-1: Clean reads are non-blocking.
/// Tests that `get_or_wait` returns immediately for non-dirty queries —
/// either because the file already has published diagnostics (clean) or
/// because it's unknown to the store (empty result).
#[rstest]
#[case::clean_published_file(
    &[("unused var", DiagnosticSeverity::WARNING)],
    "/src/main.rs",
    &["unused var"],
)]
#[case::unknown_file(&[], "/no/such/file.rs", &[])]
fn returns_immediately(
    #[case] pre_publish: &[(&str, DiagnosticSeverity)],
    #[case] query_path: &str,
    #[case] expected_messages: &[&str],
) {
    let store = DiagnosticStore::new();
    if !pre_publish.is_empty() {
        let diags = pre_publish.iter().map(|(m, s)| dummy_diagnostic(m, *s)).collect();
        store.publish(Path::new(query_path), diags);
    }
    let start = Instant::now();
    let result = store.get_or_wait(Path::new(query_path), Duration::from_secs(2));
    assert!(start.elapsed() < Duration::from_millis(10));
    let actual: Vec<_> = result.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(actual, expected_messages);
}

// SC-1 supplement: Unknown file returns empty immediately.
// SC-2: Dirty file blocks until publish.
/// Tests that a dirty file blocks readers until diagnostics are published.
#[rstest]
fn dirty_file_blocks_until_publish() {
    let store = Arc::new(DiagnosticStore::new());
    let path = Path::new("/src/lib.rs");
    store.mark_dirty(path);

    let (waited, result) = park_waiter_then_publish(&store, "/src/lib.rs", Duration::from_secs(2), path, vec![
        dummy_diagnostic("error here", DiagnosticSeverity::ERROR),
    ]);

    assert!(waited < Duration::from_millis(200), "waited {waited:?}");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].message, "error here");
}

// SC-3: Dirty file times out.
/// Tests that a dirty file returns stale diagnostics after the timeout.
#[rstest]
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
/// Tests that multiple waiters on the same file all wake on publish.
#[rstest]
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
/// Tests that publishing diagnostics for file B does not unblock a waiter on file A.
#[rstest]
fn publish_for_other_file_does_not_unblock() {
    let store = Arc::new(DiagnosticStore::new());
    let path_a = Path::new("/src/a.rs");
    let path_b = Path::new("/src/b.rs");
    store.mark_dirty(path_a);
    store.mark_dirty(path_b);

    let (waited, result) = park_waiter_then_publish(&store, "/src/a.rs", Duration::from_millis(200), path_b, vec![
        dummy_diagnostic("b diag", DiagnosticSeverity::ERROR),
    ]);

    // File A should have timed out (waited ~200ms), not unblocked early.
    assert!(waited >= Duration::from_millis(150), "unblocked too early: {waited:?}");
    assert!(result.is_empty());
}

/// Tests that removing a file clears its diagnostics.
#[rstest]
fn remove_clears_entry() {
    let store = DiagnosticStore::new();
    let path = Path::new("/src/gone.rs");

    store.publish(path, vec![dummy_diagnostic("hi", DiagnosticSeverity::HINT)]);
    assert_eq!(store.get_or_wait(path, Duration::ZERO).len(), 1);

    store.remove(path);
    assert!(store.get_or_wait(path, Duration::ZERO).is_empty());
}

/// Tests that a mark-dirty then publish cycle updates diagnostics correctly.
#[rstest]
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
