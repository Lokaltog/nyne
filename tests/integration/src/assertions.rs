//! Assertion helpers for integration tests.
//!
//! These helpers panic with descriptive messages showing captured stdout/stderr
//! so test failures are easy to debug.

use crate::command::CommandOutput;

/// Panic unless the command succeeded (exit code 0).
#[track_caller]
pub fn assert_ok(out: &CommandOutput) {
    assert!(
        out.is_ok(),
        "expected command to succeed, got exit={}\nstdout: {}\nstderr: {}",
        out.exit_code,
        out.stdout,
        out.stderr,
    );
}

/// Panic unless the command failed (non-zero exit code).
#[track_caller]
pub fn assert_fails(out: &CommandOutput) {
    assert!(
        !out.is_ok(),
        "expected command to fail, got exit=0\nstdout: {}\nstderr: {}",
        out.stdout,
        out.stderr,
    );
}

/// Panic unless `haystack` contains `needle`.
#[track_caller]
pub fn assert_contains(haystack: &str, needle: &str) {
    assert!(haystack.contains(needle), "expected to find {needle:?} in:\n{haystack}");
}

/// Panic unless `haystack` contains at least one of the given needles.
#[track_caller]
pub fn assert_contains_any(haystack: &str, needles: &[&str]) {
    assert!(
        needles.iter().any(|n| haystack.contains(n)),
        "expected one of {needles:?} in:\n{haystack}"
    );
}
