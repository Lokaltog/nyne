use rstest::rstest;

use super::*;

/// Verifies that `is_pid_alive` correctly distinguishes live from dead PIDs.
#[rstest]
#[case::current_process(std::process::id() as i32, true)]
#[case::bogus_pid(i32::MAX, false)]
fn is_pid_alive_reports(#[case] pid: i32, #[case] expected: bool) {
    assert_eq!(is_pid_alive(pid), expected);
}
