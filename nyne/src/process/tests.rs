use rstest::rstest;

use super::*;

/// Verifies that the current process PID is reported as alive.
#[rstest]
fn current_process_is_alive() {
    assert!(is_pid_alive(std::process::id() as i32));
}

/// Verifies that an invalid PID is reported as not alive.
#[rstest]
fn bogus_pid_is_not_alive() {
    assert!(!is_pid_alive(i32::MAX));
}
