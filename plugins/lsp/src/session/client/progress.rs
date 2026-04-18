//! Progress tracking for LSP server indexing lifecycle.
//!
//! Gates LSP requests behind initial workspace indexing. The first time a
//! server completes its cold-start indexing cycle (as observed via
//! `$/progress` notifications), the tracker transitions to `Ready` and
//! requests stop blocking. Subsequent transient progress (background
//! re-analysis after edits) does **not** regress the state -- once
//! indexed, always queryable.
//!
//! # State machine
//!
//! ```text
//! Uninitialized { open } --arm()--> Indexing { open } --note_end-to-empty--> Ready
//!         |                                |                                   |
//!         |                                +--wait_ready timeout---------------+
//!         |                                                                    |
//!         +--------------------------------shutdown()-------------------------> Shutdown
//! ```
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
use std::time::Duration;

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
    /// Armed and waiting for indexing to quiesce. `wait_ready` blocks
    /// here until `open` transitions non-empty -> empty, or until the
    /// caller's grace timeout elapses (in which case the caller's
    /// `wait_ready` forces the transition to `Ready` so subsequent
    /// requests do not pay the same cost).
    Indexing { open: HashSet<NumberOrString> },
    /// Initial indexing complete. `wait_ready` returns immediately.
    /// Background re-analysis after edits stays in `Ready` -- transient
    /// progress does not regress to `Indexing`.
    Ready,
    /// Terminal. Reached on `Client` shutdown. All waiters are released
    /// and new waiters return immediately so shutdown cannot deadlock
    /// against the gate.
    Shutdown,
}

/// Tracks the LSP server's indexing lifecycle and gates query requests
/// behind initial quiescence.
///
/// Thread-safe via `parking_lot::Mutex` + `Condvar`. Multiple FUSE
/// handler threads can park on the same tracker; a single state
/// transition wakes them all.
pub(super) struct ProgressTracker {
    state: Mutex<State>,
    cv: Condvar,
    server_name: String,
}

impl ProgressTracker {
    /// Create a new tracker in the `Uninitialized` state. Requests will
    /// pass through (not block) until [`arm`] is called.
    pub(super) fn new(server_name: impl Into<String>) -> Self {
        Self {
            state: Mutex::new(State::Uninitialized { open: HashSet::new() }),
            cv: Condvar::new(),
            server_name: server_name.into(),
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
        *state = State::Indexing { open };
        // Wake any waiters that parked between construction and arm
        // (none today, but cheap insurance against future call orderings).
        self.cv.notify_all();
    }

    /// Note a `$/progress` `Begin` notification. Adds the token to the
    /// tracked set in `Uninitialized` or `Indexing`. Ignored in `Ready`
    /// (background re-analysis must not regress) and `Shutdown`.
    pub(super) fn note_begin(&self, token: NumberOrString) {
        match &mut *self.state.lock() {
            State::Uninitialized { open } | State::Indexing { open } => {
                debug!(
                    target: "nyne::lsp",
                    server = %self.server_name,
                    ?token,
                    "progress begin",
                );
                open.insert(token);
            }
            State::Ready | State::Shutdown => {}
        }
    }

    /// Note a `$/progress` `End` notification. Removes the token from
    /// the tracked set. In `Indexing`, if the set transitions
    /// non-empty -> empty, transitions to `Ready` and wakes all waiters.
    pub(super) fn note_end(&self, token: &NumberOrString) {
        let mut state = self.state.lock();
        match &mut *state {
            State::Uninitialized { open } => {
                open.remove(token);
            }
            State::Indexing { open } => {
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
                if open.is_empty() {
                    info!(
                        target: "nyne::lsp",
                        server = %self.server_name,
                        "indexed",
                    );
                    *state = State::Ready;
                    self.cv.notify_all();
                }
            }
            State::Ready | State::Shutdown => {}
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
    pub(super) fn wait_ready(&self, timeout: Duration) -> bool {
        let mut state = self.state.lock();
        if !matches!(*state, State::Indexing { .. }) {
            return true;
        }
        if !self
            .cv
            .wait_while_for(&mut state, |s| matches!(s, State::Indexing { .. }), timeout)
            .timed_out()
        {
            return true;
        }
        // Grace period expired. Force to `Ready` so this caller proceeds
        // and so subsequent callers do not re-pay the timeout. Re-check
        // the state under the still-held lock: another thread may have
        // transitioned to `Ready`/`Shutdown` between the timeout firing
        // and us re-acquiring it.
        if matches!(*state, State::Indexing { .. }) {
            warn!(
                target: "nyne::lsp",
                server = %self.server_name,
                ?timeout,
                "indexing grace period expired; marking ready (queries will proceed without waiting)",
            );
            *state = State::Ready;
            self.cv.notify_all();
        }
        false
    }

    /// Return `true` if the tracker has completed initial indexing.
    /// Diagnostics / testing only.
    #[cfg(test)]
    pub(super) fn is_ready(&self) -> bool { matches!(*self.state.lock(), State::Ready) }
}

#[cfg(test)]
mod tests;
