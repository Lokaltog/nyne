//! Category 18 — Rename preview diff validation (T-1700..T-1701).
//!
//! Verifies that LSP rename previews contain project-wide changes in a valid
//! unified diff format, and that reading them is idempotent (no mutation).

use nyne_integration_tests::targets::lsp::{FILE, SYMBOL};
use nyne_integration_tests::{NyneMount, assert_contains, mount};
use rstest::rstest;

/// T-1700: Rename preview contains project-wide changes in valid diff format.
#[rstest]
fn t_1700_rename_preview_project_wide(mount: NyneMount) {
    let diff = mount.read(&format!("{FILE}@/symbols/{SYMBOL}@/rename/TestRenamedProvider.diff"));
    assert_contains(&diff, "TestRenamedProvider");
    assert_contains(&diff, "--- a/");
    assert_contains(&diff, "+++ b/");
    assert!(
        diff.matches("--- a/").count() >= 2,
        "rename should touch multiple files:\n{diff}"
    );
}

/// T-1701: Rename preview read is idempotent — no source mutation.
#[rstest]
fn t_1701_rename_preview_idempotent(mount: NyneMount) {
    let diff_path = format!("{FILE}@/symbols/{SYMBOL}@/rename/AnotherName.diff");
    let diff1 = mount.read(&diff_path);
    let raw_before = mount.read(FILE);
    let diff2 = mount.read(&diff_path);
    let raw_after = mount.read(FILE);

    assert_eq!(diff1, diff2, "rename preview should be idempotent");
    assert_eq!(
        raw_before, raw_after,
        "source file must not be mutated by reading rename preview"
    );
}
