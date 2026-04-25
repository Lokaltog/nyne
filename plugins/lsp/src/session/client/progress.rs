//! Progress tracking for LSP server indexing lifecycle.
//!
//! Gates LSP requests behind initial workspace indexing. The tracker
//! transitions to `Ready` once the open progress set has been
//! continuously empty for a configurable `debounce` window, after which
//! requests stop blocking. Subsequent transient progress (background
//! re-analysis after edits) does **not** regress the state -- once
//! indexed, always queryable.
//!
//! # State machine
//!
//! ```text
//! Uninitialized { open }
//!         |
//!         | arm()
//!         v
//! Indexing { open, idle_since }
//!         |                        ^
//!         | wait_ready: idle_since |  note_begin (resets idle_since)
//!         | elapsed >= debounce    |
//!         v                        |
//!       Ready  <---wait_ready timeout (grace)---+
//!         |
//!         | shutdown()
//!         v
//!    Shutdown   (terminal; distinct from Ready)
//! ```
//!
//! ## Why a debounce, not an immediate transition
//!
//! Rust-analyzer (and other servers) emit multiple sequential `$/progress`
//! cycles -- e.g. `rustAnalyzer/Fetching` for cargo metadata, then
//! `rustAnalyzer/Indexing` for semantic analysis. An immediate
//! `note_end`-to-empty -> `Ready` transition releases the gate between
//! the two cycles, so reads land before semantic indexing actually
//! starts and rust-analyzer returns empty results. The debounce window
//! absorbs that gap: any new `Begin` arriving before the window elapses
//! resets `idle_since`, keeping the gate parked through the next cycle.
//!
//! ## Why an enum, not a boolean
//!
//! A boolean conflates "not gating yet" (pre-arm) with "gate is open"
//! (ready), and provides no place to track tokens that arrive between
//! `initialize` returning and `arm` being called. The enum encodes
//! lifecycle phases distinctly, makes the early-`Begin` race
//! impossible (tokens accumulate in `Uninitialized` and are carried
//! into `Indexing` by `arm`), and makes shutdown-wakes-waiters
//! explicit (a terminal state, distinct from `Ready`).

use std::collections::HashSet;
use std::mem;
use std::time::{Duration, Instant};

use lsp_types::NumberOrString;
use parking_lot::{Condvar, Mutex};
use tracing::{debug, info, warn};

/// Lifecycle state of the LSP server's initial indexing.
#[derive(Debug)]
enum State {
    /// Pre-arm. `note_begin`/`note_end` track tokens but `wait_ready`
    /// returns immediately. This window covers the time between
    /// `Client::spawn` starting the reader thread and `arm` being
    /// called after the `initialize` handshake completes -- any
    /// `$/progress` `Begin` that arrives in that window is preserved
    /// and carried into `Indexing` by `arm`.
    Uninitialized { open: HashSet<NumberOrString> },
    /// Armed and waiting for indexing to quiesce. The inner type
    /// encodes the open-set/debounce-clock invariant: `Active` carries
    /// a non-empty open set with no clock; `Quiescent` carries a
    /// quiescence clock with no open tokens. `wait_ready` releases
    /// the gate once a `Quiescent` has held for the configured
    /// debounce window without a new `Begin` flipping it back to
    /// `Active`.
    Indexing(Indexing),
    /// Initial indexing complete. `wait_ready` returns immediately.
    /// Background re-analysis after edits stays in `Ready` -- transient
    /// progress does not regress to `Indexing`.
    Ready,
    /// Terminal. Reached on `Client` shutdown. All waiters are released
    /// and new waiters return immediately so shutdown cannot deadlock
    /// against the gate.
    Shutdown,
}

/// Sub-state of [`State::Indexing`].
///
/// Encoding the open-set/clock invariant in the type means the
/// notification handlers and `wait_ready` cannot drift out of sync
/// (open empty + no clock, or open non-empty + clock running, are
/// both unrepresentable). The debounce avoids transitioning
/// prematurely between sequential progress cycles (e.g.
/// rust-analyzer emits `rustAnalyzer/Fetching` then
/// `rustAnalyzer/Indexing` -- a `note_end`-to-empty -> `Ready`
/// transition without a window would release the gate in the gap).
#[derive(Debug)]
enum Indexing {
    /// Progress is in flight. `wait_ready` blocks until `open` drains.
    Active { open: HashSet<NumberOrString> },
    /// Open set has been empty since `since`. `wait_ready` transitions
    /// to `Ready` once `since.elapsed() >= debounce`. A new `Begin`
    /// arriving here flips back to `Active`.
    Quiescent { since: Instant },
}

/// Tracks the LSP server's indexing lifecycle and gates query requests
/// behind initial quiescence.
///
/// Thread-safe via `parking_lot::Mutex` + `Condvar`. Multiple FUSE
/// handler threads can park on the same tracker; a single state
/// transition wakes them all.
///
/// `debounce` is the minimum time the open progress set must remain
/// empty before `wait_ready` transitions the tracker to `Ready`. This
/// absorbs the gap between sequential progress cycles emitted by some
/// servers (rust-analyzer in particular) so reads do not land between
/// the metadata-fetch phase and the semantic-indexing phase.
pub(super) struct ProgressTracker {
    state: Mutex<State>,
    cv: Condvar,
    server_name: String,
    debounce: Duration,
}

impl ProgressTracker {
    /// Create a new tracker in the `Uninitialized` state. Requests will
    /// pass through (not block) until [`arm`] is called.
    ///
    /// `debounce` is the minimum time the open progress set must
    /// remain empty before `wait_ready` releases the gate to `Ready`.
    /// See the struct-level docs for rationale.
    pub(super) fn new(server_name: impl Into<String>, debounce: Duration) -> Self {
        Self {
            state: Mutex::new(State::Uninitialized { open: HashSet::new() }),
            cv: Condvar::new(),
            server_name: server_name.into(),
            debounce,
        }
    }

    /// Transition `Uninitialized -> Indexing`, preserving any in-flight
    /// progress tokens. No-op in any other state.
    ///
    /// Called by `Client::spawn` immediately after `initialize` returns
    /// successfully, before any FUSE-driven request can reach the
    /// client. Tokens that arrived between the start of the reader
    /// thread and this call are carried into `Indexing` so the
    /// transition to `Ready` waits for them to close.
    pub(super) fn arm(&self) {
        let mut state = self.state.lock();
        let State::Uninitialized { open } = &mut *state else {
            return;
        };
        let open = mem::take(open);
        debug!(
            target: "nyne::lsp",
            server = %self.server_name,
            in_flight_tokens = open.len(),
            "armed indexing tracker",
        );
        // No in-flight tokens at arm time -> start the debounce clock
        // immediately. Servers that emit no progress at all (or whose
        // only Begin/End cycle landed during `Uninitialized`) reach
        // `Ready` via the debounce path rather than the grace timeout.
        *state = if open.is_empty() {
            State::Indexing(Indexing::Quiescent { since: Instant::now() })
        } else {
            State::Indexing(Indexing::Active { open })
        };
        // Wake any waiters that parked between construction and arm
        // (none today, but cheap insurance against future call orderings).
        self.cv.notify_all();
    }

    /// Note a `$/progress` `Begin` notification.
    ///
    /// In `Uninitialized`, accumulates the token. In `Indexing`, adds
    /// the token to the open set; if currently `Quiescent`, flips back
    /// to `Active` (the debounce clock is reset because a new cycle
    /// has started). Ignored in `Ready` (background re-analysis must
    /// not regress) and `Shutdown`.
    pub(super) fn note_begin(&self, token: NumberOrString) {
        debug!(target: "nyne::lsp", server = %self.server_name, ?token, "progress begin");
        match &mut *self.state.lock() {
            State::Uninitialized { open } | State::Indexing(Indexing::Active { open }) => {
                open.insert(token);
            }
            State::Indexing(idx) => {
                let mut open = HashSet::new();
                open.insert(token);
                *idx = Indexing::Active { open };
            }
            State::Ready | State::Shutdown => {}
        }
    }

    /// Note a `$/progress` `End` notification. Removes the token from
    /// the tracked set. In `Indexing::Active`, if the set transitions
    /// non-empty -> empty, flips to `Quiescent { since: now }` and
    /// wakes waiters; the actual `Indexing -> Ready` transition is
    /// performed in `wait_ready` once the debounce window has elapsed
    /// without a new `Begin`.
    pub(super) fn note_end(&self, token: &NumberOrString) {
        let mut state = self.state.lock();
        let drained_to_empty = match &mut *state {
            State::Uninitialized { open } => {
                open.remove(token);
                false
            }
            State::Indexing(Indexing::Active { open }) => {
                if !open.remove(token) {
                    return;
                }
                debug!(
                    target: "nyne::lsp",
                    server = %self.server_name,
                    ?token,
                    remaining = open.len(),
                    "progress end",
                );
                open.is_empty()
            }
            // `End` against a Quiescent or post-Indexing state is a
            // no-op: either the token was already drained (Quiescent),
            // or the gate has moved past the initial cycle (Ready /
            // Shutdown -- background re-analysis must not regress).
            State::Indexing(Indexing::Quiescent { .. }) | State::Ready | State::Shutdown => false,
        };
        if drained_to_empty {
            *state = State::Indexing(Indexing::Quiescent { since: Instant::now() });
            // Wake waiters so they can re-check and start the debounce
            // wait. The transition to `Ready` happens in `wait_ready`
            // after the debounce window passes without a new `Begin`
            // flipping the state back to `Active`.
            self.cv.notify_all();
        }
    }

    /// Transition to `Shutdown` from any state, releasing all waiters.
    ///
    /// Called by `Client::shutdown` before issuing the LSP `shutdown`
    /// request, so the request itself cannot block on the gate.
    /// Idempotent.
    pub(super) fn shutdown(&self) {
        let mut state = self.state.lock();
        if matches!(*state, State::Shutdown) {
            return;
        }
        debug!(
            target: "nyne::lsp",
            server = %self.server_name,
            "tracker shutdown",
        );
        *state = State::Shutdown;
        self.cv.notify_all();
    }

    /// Block until the tracker is queryable, or until `timeout` elapses.
    ///
    /// Returns `true` if the tracker is in a non-blocking state on
    /// return (`Uninitialized`, `Ready`, or `Shutdown`); `false` if
    /// the wait timed out while still in `Indexing`. In the timeout
    /// case, this method **forces the transition to `Ready`** before
    /// returning, so subsequent requests do not pay the same cost --
    /// this is the inline grace-timer behavior. Callers proceed in
    /// either case; the boolean is informational (used by tests and
    /// for diagnostic logging).
    ///
    /// The `Indexing -> Ready` transition is performed here (not in
    /// `note_end`) once the open progress set has been continuously
    /// empty for the configured `debounce` window. Any new `Begin`
    /// arriving inside that window flips `Quiescent -> Active` via
    /// `note_begin`, so the gate stays parked until the server
    /// actually quiesces.
    pub(super) fn wait_ready(&self, timeout: Duration) -> bool {
        let mut state = self.state.lock();
        // Steady-state short-circuit before any clock or loop work.
        if !matches!(*state, State::Indexing(_)) {
            return true;
        }
        let start = Instant::now();
        loop {
            // Single deadline check for the whole function: every
            // path through the loop forces `Ready` on timeout, so
            // centralizing the comparison keeps the arm bodies flat.
            // `filter(!is_zero)` handles the exact-deadline boundary
            // (Duration::ZERO would otherwise spin one extra round).
            let Some(remaining) = timeout.checked_sub(start.elapsed()).filter(|d| !d.is_zero()) else {
                self.force_ready(&mut state, timeout);
                return false;
            };
            // Quiescent -> Ready transition: lifted above the match so
            // the match arms are pure "what to wait on", not "what to
            // wait on plus what to do after." A let-chain keeps the
            // predicate flat without an extra block level.
            if let State::Indexing(Indexing::Quiescent { since }) = &*state
                && since.elapsed() >= self.debounce
            {
                info!(target: "nyne::lsp", server = %self.server_name, "indexed");
                *state = State::Ready;
                self.cv.notify_all();
                return true;
            }
            match &*state {
                State::Uninitialized { .. } | State::Ready | State::Shutdown => return true,
                // Active: pure wait. Any post-wait outcome (timeout,
                // moved to Quiescent, moved to Ready, shutdown) is
                // handled by the next iteration's deadline check and
                // hoisted transition above — so no `if .timed_out()`
                // branch here.
                State::Indexing(Indexing::Active { .. }) => {
                    self.cv.wait_while_for(
                        &mut state,
                        |s| matches!(s, State::Indexing(Indexing::Active { .. })),
                        remaining,
                    );
                }
                // Quiescent: wait the remainder of the debounce window,
                // capped by the grace deadline. A new `Begin` flips
                // `Quiescent -> Active` and breaks the predicate early.
                // `saturating_sub` is safe-by-construction (the hoisted
                // `elapsed >= debounce` guard above already returned).
                State::Indexing(Indexing::Quiescent { since }) => {
                    // Compute elapsed first so the immutable borrow on
                    // `state` ends before the `&mut state` below.
                    let elapsed = since.elapsed();
                    self.cv.wait_while_for(
                        &mut state,
                        |s| matches!(s, State::Indexing(Indexing::Quiescent { .. })),
                        self.debounce.saturating_sub(elapsed).min(remaining),
                    );
                }
            }
        }
    }

    /// Force-transition to `Ready` from `Indexing`, logging the grace
    /// expiry. Caller-held lock is reused. No-op if another thread
    /// already advanced the state.
    fn force_ready(&self, state: &mut State, timeout: Duration) {
        if matches!(*state, State::Indexing(_)) {
            warn!(
                target: "nyne::lsp",
                server = %self.server_name,
                ?timeout,
                "indexing grace period expired; marking ready (queries will proceed without waiting)",
            );
            *state = State::Ready;
            self.cv.notify_all();
        }
    }

    /// Return `true` if the tracker has completed initial indexing.
    /// Diagnostics / testing only.
    #[cfg(test)]
    pub(super) fn is_ready(&self) -> bool { matches!(*self.state.lock(), State::Ready) }
}

#[cfg(test)]
mod tests;
