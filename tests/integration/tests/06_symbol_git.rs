//! Category 6 — Per-symbol git features (T-500..T-502).
//!
//! Validates git provider nodes scoped to individual symbols: blame, log,
//! and historical body versions.

use nyne_integration_tests::targets::rust::{FILE, SYMBOL};
use nyne_integration_tests::{NyneMount, assert_contains, assert_ok, mount};
use rstest::rstest;

/// T-500: `git/BLAME.md` — symbol-scoped blame has entries.
#[rstest]
fn t_500_symbol_blame(mount: NyneMount) {
    let blame = mount.read(&format!("{FILE}@/symbols/{SYMBOL}@/git/BLAME.md"));
    assert_contains(&blame, "Hash");
    assert_contains(&blame, "Author");
}

/// T-501: `git/LOG.md` — symbol-scoped log has at least one commit.
#[rstest]
fn t_501_symbol_log(mount: NyneMount) {
    assert_contains(&mount.read(&format!("{FILE}@/symbols/{SYMBOL}@/git/LOG.md")), "Hash");
}

/// T-502: `git/history/` — historical symbol body directory exists.
#[rstest]
fn t_502_symbol_history(mount: NyneMount) {
    assert_ok(&mount.sh(&format!("ls {FILE}@/symbols/{SYMBOL}@/git/history/")));
}
