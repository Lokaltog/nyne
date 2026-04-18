use std::thread;
use std::time::{Duration, Instant};

use lsp_types::NumberOrString;
use rstest::rstest;

use super::*;

fn token(s: &str) -> NumberOrString { NumberOrString::String(s.to_owned()) }

/// Result of a wall-clock-measured call. Returned by [`measure`].
struct Measured<R> {
    elapsed: Duration,
    result: R,
}

impl<R> Measured<R> {
    /// Run `f` and capture elapsed time since `start`. Struct-field
    /// initializers evaluate in source order, so `result: f()` runs
    /// before `elapsed: start.elapsed()`.
    fn from_start(start: Instant, f: impl FnOnce() -> R) -> Self {
        Self {
            result: f(),
            elapsed: start.elapsed(),
        }
    }
}

/// Run `f` and capture wall-clock elapsed alongside its return value.
fn measure<R>(f: impl FnOnce() -> R) -> Measured<R> { Measured::from_start(Instant::now(), f) }

/// Assert `wait_ready(timeout)` returns `ok=true` within `bound`.
fn assert_returns_within(t: &ProgressTracker, timeout: Duration, bound: Duration) {
    let Measured { elapsed, result: ok } = measure(|| t.wait_ready(timeout));
    assert!(
        ok && elapsed < bound,
        "wait_ready: ok={ok} elapsed={elapsed:?}, expected ok=true && elapsed < {bound:?}",
    );
}

/// Assert `wait_ready(timeout)` blocks at least until `timeout` and
/// reports `ok=false`.
fn assert_blocks_until(t: &ProgressTracker, timeout: Duration) {
    let Measured { elapsed, result: ok } = measure(|| t.wait_ready(timeout));
    assert!(
        !ok && elapsed >= timeout,
        "wait_ready: ok={ok} elapsed={elapsed:?}, expected ok=false && elapsed >= {timeout:?}",
    );
}

/// Operations applied to a tracker in declarative test scripts.
#[derive(Debug, Clone, Copy)]
enum Op {
    Arm,
    Begin(&'static str),
    End(&'static str),
    Shutdown,
}

fn apply(t: &ProgressTracker, ops: &[Op]) {
    for op in ops {
        match op {
            Op::Arm => t.arm(),
            Op::Begin(s) => t.note_begin(token(s)),
            Op::End(s) => t.note_end(&token(s)),
            Op::Shutdown => t.shutdown(),
        }
    }
}

/// Expected lifecycle status after replaying a script. `Ready` means
/// the tracker has reached the `Ready` state (queryable). `Pending`
/// means anything else -- `Uninitialized`, `Indexing`, or `Shutdown`.
/// We use a custom enum (not `bool`) so failing test cases identify
/// the expected state by name in the diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expect {
    Ready,
    Pending,
}

impl Expect {
    fn matches(self, t: &ProgressTracker) -> bool { (self == Expect::Ready) == t.is_ready() }
}

/// State-machine transitions: declarative `(operations -> expected status)`.
///
/// `Expect::Ready` means `is_ready` returns true. `Expect::Pending` is
/// any other state -- this also pins down that `Shutdown` is distinct
/// from `Ready`.
#[rstest]
#[case::begin_then_end_transitions_to_ready(
    &[Op::Arm, Op::Begin("a"), Op::End("a")],
    Expect::Ready,
)]
#[case::early_begin_carried_across_arm(
    &[Op::Begin("early"), Op::Arm, Op::End("early")],
    Expect::Ready,
)]
#[case::partial_quiescence_blocks(
    &[Op::Arm, Op::Begin("a"), Op::Begin("b"), Op::End("a")],
    Expect::Pending,
)]
#[case::full_quiescence_ready(
    &[Op::Arm, Op::Begin("a"), Op::Begin("b"), Op::End("a"), Op::End("b")],
    Expect::Ready,
)]
#[case::end_in_ready_does_not_regress(
    &[Op::Arm, Op::Begin("a"), Op::End("a"), Op::Begin("b"), Op::End("b")],
    Expect::Ready,
)]
#[case::begin_in_ready_does_not_regress(
    &[Op::Arm, Op::Begin("a"), Op::End("a"), Op::Begin("b")],
    Expect::Ready,
)]
#[case::end_for_unknown_token_is_no_op(&[Op::Arm, Op::End("ghost")], Expect::Pending)]
#[case::end_during_uninitialized_then_arm(&[Op::End("ghost"), Op::Arm], Expect::Pending)]
#[case::arm_is_idempotent_preserves_tokens(
    &[Op::Arm, Op::Begin("a"), Op::Arm],
    Expect::Pending,
)]
#[case::shutdown_is_terminal_not_ready(
    &[Op::Arm, Op::Begin("a"), Op::End("a"), Op::Shutdown],
    Expect::Pending,
)]
#[case::shutdown_idempotent(&[Op::Shutdown, Op::Shutdown], Expect::Pending)]
fn state_transitions(#[case] ops: &[Op], #[case] expected: Expect) {
    let t = ProgressTracker::new("test");
    apply(&t, ops);
    assert!(
        expected.matches(&t),
        "expected {expected:?} after {ops:?}, is_ready={}",
        t.is_ready()
    );
}

/// `wait_ready` returns immediately (no condvar park) in every state
/// except `Indexing`. The 50 ms ceiling catches any accidental block.
#[rstest]
#[case::uninitialized(&[])]
#[case::ready(&[Op::Arm, Op::Begin("a"), Op::End("a")])]
#[case::shutdown_from_fresh(&[Op::Shutdown])]
#[case::shutdown_from_indexing(&[Op::Arm, Op::Begin("a"), Op::Shutdown])]
#[case::shutdown_from_ready(&[Op::Arm, Op::Begin("a"), Op::End("a"), Op::Shutdown])]
fn wait_ready_passes_through_in_non_blocking_states(#[case] before: &[Op]) {
    let t = ProgressTracker::new("test");
    apply(&t, before);
    assert_returns_within(&t, Duration::from_secs(1), Duration::from_millis(50));
}

/// Inline grace-timer behavior: when the tracker is armed but no
/// progress arrives (or never closes), `wait_ready` blocks until the
/// configured timeout, then forces the transition to `Ready` so
/// subsequent callers proceed without re-paying the cost.
#[test]
fn arm_with_no_tokens_blocks_until_grace_timeout_then_forces_ready() {
    let t = ProgressTracker::new("test");
    t.arm();
    assert_blocks_until(&t, Duration::from_millis(80));
    assert!(t.is_ready(), "grace-period expiry must force Ready");
    assert_returns_within(&t, Duration::from_secs(1), Duration::from_millis(50));
}

/// Shutdown wakes any thread parked on `wait_ready` and returns the
/// `ready=true` signal so callers proceed (and the LSP `shutdown`
/// request itself never deadlocks against the gate).
#[test]
fn shutdown_releases_parked_waiters() {
    let t = ProgressTracker::new("test");
    t.arm();
    t.note_begin(token("a"));

    thread::scope(|s| {
        s.spawn(|| {
            assert_returns_within(&t, Duration::from_secs(5), Duration::from_millis(500));
        });
        // Give the waiter time to park on the condvar.
        thread::sleep(Duration::from_millis(30));
        t.shutdown();
        // Scope auto-joins; panics in the spawned thread propagate here.
    });
}

/// Multiple FUSE handler threads that park on the same tracker all wake
/// on a single state transition. Verifies the condvar `notify_all`
/// fan-out and that no waiter starves.
#[test]
fn concurrent_waiters_all_wake_on_quiescence() {
    let t = ProgressTracker::new("test");
    t.arm();
    t.note_begin(token("a"));

    thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| {
                assert!(t.wait_ready(Duration::from_secs(5)), "all concurrent waiters must wake");
            });
        }
        thread::sleep(Duration::from_millis(30));
        t.note_end(&token("a"));
    });
}

/// When the grace timeout fires under contention, exactly one waiter
/// observes `Indexing` under the lock and forces the transition. The
/// other waiter either times out independently and sees `Ready` (and
/// returns `false`) or is woken early by `notify_all` from the
/// force-ready transition (and returns `true`). Both interleavings
/// are valid; what we pin down is:
/// 1. No panic / no double-transition (scope auto-joins).
/// 2. Post-condition is `Ready`.
/// 3. At least one waiter reports timeout (the one that observed the
///    expiry under the lock).
#[test]
fn concurrent_grace_timeout_resolves_consistently() {
    let t = ProgressTracker::new("test");
    t.arm();
    t.note_begin(token("a"));

    let timed_out = std::sync::atomic::AtomicUsize::new(0);
    thread::scope(|s| {
        for _ in 0..2 {
            s.spawn(|| {
                if !t.wait_ready(Duration::from_millis(60)) {
                    timed_out.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            });
        }
    });
    assert!(t.is_ready(), "post-condition: grace force-readied the tracker");
    assert!(
        timed_out.load(std::sync::atomic::Ordering::Relaxed) >= 1,
        "at least one waiter must observe the grace expiry, got {}",
        timed_out.load(std::sync::atomic::Ordering::Relaxed),
    );
}
