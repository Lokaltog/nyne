use rstest::rstest;

use super::*;

/// Verifies `read_comm` and `read_ppid` both return `None` for a PID that cannot exist.
#[rstest]
#[case::read_comm(|| read_comm(u32::MAX).is_none())]
#[case::read_ppid(|| read_ppid(u32::MAX).is_none())]
fn procfs_readers_return_none_for_nonexistent_pid(#[case] check: fn() -> bool) {
    assert!(check());
}

/// Verifies `read_comm` returns a non-empty, length-bounded name for the current process.
#[rstest]
fn read_comm_of_current_process_has_name() {
    let comm = read_comm(std::process::id()).expect("current process must have comm");
    assert!(!comm.is_empty());
    assert!(comm.len() <= COMM_MAX_LEN);
}

/// Verifies `read_ppid` returns a positive parent PID for the current process.
#[rstest]
fn read_ppid_of_current_process_is_positive() {
    assert!(read_ppid(std::process::id()).is_some_and(|p| p > 0));
}

/// Verifies truncation behavior: short names borrow, long names get truncated to
/// `COMM_MAX_LEN`, and truncation respects UTF-8 character boundaries.
#[rstest]
#[case::borrows_short("bash", "bash", true)]
#[case::owns_long("very_long_process_name_here", "very_long_proce", false)]
#[case::respects_utf8_boundaries("abcdefghijklmno香", "abcdefghijklmno", false)]
fn truncate_comm_cases(#[case] input: &str, #[case] expected: &str, #[case] is_borrowed: bool) {
    let result = truncate_comm(input);
    assert_eq!(result, expected);
    assert!(result.len() <= COMM_MAX_LEN);
    assert_eq!(matches!(result, Cow::Borrowed(_)), is_borrowed);
}
