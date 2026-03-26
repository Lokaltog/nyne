//! Process liveness checks for session staleness detection.
//!
//! Provides a `kill(pid, 0)` equivalent via `rustix` for checking whether
//! a daemon process is still running. The session registry uses this to
//! prune session files whose daemons have exited.

use rustix::process::{Pid, test_kill_process};

/// Check whether a process is alive (equivalent to `kill(pid, 0)`).
///
/// Returns `false` for invalid PIDs (zero or negative values that
/// `Pid::from_raw` rejects). Used by the session registry to prune
/// stale session files whose daemon has exited.
pub fn is_pid_alive(pid: i32) -> bool {
    let Some(pid) = Pid::from_raw(pid) else {
        return false;
    };
    test_kill_process(pid).is_ok()
}
