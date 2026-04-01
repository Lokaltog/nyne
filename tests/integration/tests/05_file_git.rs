//! Category 5 — File-level git features (T-400..T-409).
//!
//! Validates git provider nodes per file: blame, log, contributors, notes,
//! sliced variants, HEAD and ref diffs, and historical versions.

use nyne_integration_tests::targets::rust::FILE;
use nyne_integration_tests::{NyneMount, assert_contains, assert_contains_any, assert_ok, mount};
use rstest::rstest;

/// T-400: `git/BLAME.md` — per-line authorship with hash and author columns.
#[rstest]
fn t_400_blame(mount: NyneMount) {
    let blame = mount.read(&format!("{FILE}@/git/BLAME.md"));
    assert_contains(&blame, "Lines");
    assert_contains(&blame, "Hash");
    assert_contains(&blame, "Author");
}

/// T-401: `git/LOG.md` — commit log with hash, author, date.
#[rstest]
fn t_401_log(mount: NyneMount) {
    let log = mount.read(&format!("{FILE}@/git/LOG.md"));
    assert_contains(&log, "Hash");
    assert_contains(&log, "Author");
    assert_contains(&log, "Date");
}

/// T-402: `git/CONTRIBUTORS.md` — contributor ranking with author and commit count.
#[rstest]
fn t_402_contributors(mount: NyneMount) {
    let contrib = mount.read(&format!("{FILE}@/git/CONTRIBUTORS.md"));
    assert_contains(&contrib, "Author");
    assert_contains(&contrib, "Commits");
}

/// T-403: `git/NOTES.md` — git notes node is readable.
#[rstest]
fn t_403_notes(mount: NyneMount) { assert_ok(&mount.sh(&format!("cat {FILE}@/git/NOTES.md"))); }

/// T-404: `git/BLAME.md:M-N` — range-sliced blame is non-empty.
#[rstest]
fn t_404_blame_range(mount: NyneMount) {
    let out = mount.sh(&format!("cat {FILE}@/git/BLAME.md:1-3"));
    assert_ok(&out);
    assert!(!out.stdout.trim().is_empty(), "expected blame entries in range 1-3");
}

/// T-405: `git/LOG.md:-N` — last N log entries.
#[rstest]
fn t_405_log_last_n(mount: NyneMount) { assert_ok(&mount.sh(&format!("cat {FILE}@/git/LOG.md:-5"))); }

/// T-406: `git/LOG.md:M-N` — range-sliced log.
#[rstest]
fn t_406_log_range(mount: NyneMount) { assert_ok(&mount.sh(&format!("cat {FILE}@/git/LOG.md:1-3"))); }

/// T-407: `diff/HEAD.diff` — uncommitted changes. Under snapshot storage the
/// mount is clean, so expect a "No changes" sentinel or a valid diff.
#[rstest]
fn t_407_diff_head(mount: NyneMount) {
    assert_contains_any(&mount.read(&format!("{FILE}@/diff/HEAD.diff")), &[
        "No changes",
        "diff --git",
        "---",
    ]);
}

/// T-408: `diff/<ref>.diff` — diff against an arbitrary ref resolves.
/// Uses the first branch from `@/git/branches/` to guarantee a valid ref.
#[rstest]
fn t_408_diff_ref(mount: NyneMount) {
    // Resolve first available branch name at runtime.
    assert_ok(&mount.sh(&format!(
        "branch=$(ls @/git/branches/ | head -1); \
         cat {FILE}@/diff/${{branch}}.diff"
    )));
}

/// T-409: `history/` — historical file versions are listed.
#[rstest]
fn t_409_history(mount: NyneMount) {
    let list = mount.sh(&format!("ls {FILE}@/history/"));
    assert_ok(&list);
    assert!(
        !list.stdout.trim().is_empty(),
        "expected at least one historical version"
    );

    // Read the first historical version — should be non-empty Rust source.
    let first = mount.sh(&format!("cat {FILE}@/history/$(ls {FILE}@/history/ | head -1)"));
    assert_ok(&first);
    assert_contains_any(&first.stdout, &["use ", "fn ", "struct ", "mod ", "impl "]);
}
