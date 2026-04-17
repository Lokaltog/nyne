use super::*;

/// Verifies `read_comm` returns a non-empty name for the current process.
#[test]
fn read_comm_of_current_process_has_name() {
    let comm = read_comm(std::process::id()).expect("current process must have comm");
    assert!(!comm.is_empty());
    assert!(comm.len() <= COMM_MAX_LEN);
}

/// Verifies `read_comm` returns `None` for a PID that cannot exist.
#[test]
fn read_comm_returns_none_for_nonexistent_pid() {
    assert_eq!(read_comm(u32::MAX), None);
}

/// Verifies `read_ppid` returns a positive parent PID for the current process.
#[test]
fn read_ppid_of_current_process_is_positive() {
    assert!(read_ppid(std::process::id()).is_some_and(|p| p > 0));
}

/// Verifies `read_ppid` returns `None` for a PID that cannot exist.
#[test]
fn read_ppid_returns_none_for_nonexistent_pid() {
    assert_eq!(read_ppid(u32::MAX), None);
}

/// Verifies short names are returned as `Cow::Borrowed` (no allocation).
#[test]
fn truncate_comm_borrows_short_name() {
    let result = truncate_comm("bash");
    assert_eq!(result, "bash");
    assert!(matches!(result, Cow::Borrowed(_)));
}

/// Verifies long names are truncated to `COMM_MAX_LEN` bytes.
#[test]
fn truncate_comm_owns_long_name() {
    let result = truncate_comm("very_long_process_name_here");
    assert_eq!(result.len(), COMM_MAX_LEN);
    assert!(matches!(result, Cow::Owned(_)));
}

/// Verifies truncation respects UTF-8 character boundaries.
#[test]
fn truncate_comm_respects_utf8_boundaries() {
    // 15 ASCII bytes followed by a 3-byte UTF-8 char — naive truncation at
    // byte 15 would split the multi-byte character.
    let result = truncate_comm("abcdefghijklmno香");
    assert_eq!(result, "abcdefghijklmno");
    assert!(result.len() <= COMM_MAX_LEN);
}
