//! Category 15 — Directory-level features (T-1400..T-1403).
//!
//! Validates that directory companions expose a directory-level OVERVIEW.md
//! and do not leak file-specific nodes (symbols/, git/BLAME.md, etc.).

use nyne_integration_tests::{NyneMount, assert_contains, mount};
use rstest::rstest;

const TEST_DIR: &str = "nyne/src/cli";

/// T-1400: Directory `@/OVERVIEW.md` is non-empty.
#[rstest]
fn t_1400_directory_overview(mount: NyneMount) {
    let content = mount.read(&format!("{TEST_DIR}/@/OVERVIEW.md"));
    assert!(!content.trim().is_empty(), "directory overview should be non-empty");
}

/// T-1401: Directory `@/` companion listing exposes OVERVIEW.md.
#[rstest]
fn t_1401_directory_companion_lists_overview(mount: NyneMount) {
    assert_contains(&mount.sh_ok(&format!("ls {TEST_DIR}/@/")), "OVERVIEW.md");
}

/// T-1402: Directory `@/` does not expose file-specific `symbols/` directory.
#[rstest]
fn t_1402_no_file_symbols_in_dir_companion(mount: NyneMount) {
    assert!(
        !mount.sh_ok(&format!("ls {TEST_DIR}/@/")).contains("symbols"),
        "directory companion should not expose file-specific symbols/"
    );
}

/// T-1403: Directory `@/OVERVIEW.md` lists source files with descriptions.
#[rstest]
fn t_1403_directory_overview_lists_files(mount: NyneMount) {
    let content = mount.read(&format!("{TEST_DIR}/@/OVERVIEW.md"));
    assert_contains(&content, "mod.rs");
    assert_contains(&content, "File");
    assert_contains(&content, "Language");
}
