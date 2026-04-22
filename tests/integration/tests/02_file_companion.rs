//! Category 2 — File-level companion features (T-100..T-107).
//!
//! Validates VFS nodes produced for each source file: OVERVIEW.md, DIAGNOSTICS.md,
//! line-addressed reads, and the symbols companion directory.

use nyne_integration_tests::targets::rust::FILE;
use nyne_integration_tests::{NyneMount, assert_contains, assert_contains_any, assert_ok, mount};
use rstest::rstest;

/// T-100: `file@/OVERVIEW.md` — file-level overview has symbol table columns.
#[rstest]
fn t_100_file_overview(mount: NyneMount) {
    let out = mount.read(&format!("{FILE}@/OVERVIEW.md"));
    for col in &["Symbol", "Kind", "Lines", "Tokens"] {
        assert_contains(&out, col);
    }
    // At least two symbol rows — Cli and Command exist in this file.
    assert_contains(&out, "Cli");
    assert_contains(&out, "Command");
}

/// T-101: `file@/symbols/OVERVIEW.md` — symbol table lists known symbols.
#[rstest]
fn t_101_symbols_overview(mount: NyneMount) {
    let out = mount.read(&format!("{FILE}@/symbols/OVERVIEW.md"));
    assert_contains(&out, "Symbol");
    assert_contains(&out, "Kind");
    assert_contains(&out, "Cli");
    assert_contains(&out, "Command");
}

/// T-102: `file@/DIAGNOSTICS.md` — LSP diagnostics node is readable.
#[rstest]
fn t_102_file_diagnostics(mount: NyneMount) {
    mount.cat_contains_any(&format!("{FILE}@/DIAGNOSTICS.md"), &[
        "Diagnostics",
        "diagnostics",
        "No diagnostics",
        "No issues",
    ]);
}

/// T-103: `file@/lines` — full file content equals raw file byte-for-byte.
#[rstest]
fn t_103_lines_full_file(mount: NyneMount) { mount.sh_ok(&format!("diff <(cat {FILE}) <(cat {FILE}@/lines)")); }

/// T-104: `file@/lines:M-N` — line range read matches `head -N` on the raw file.
#[rstest]
fn t_104_lines_range(mount: NyneMount) { mount.sh_ok(&format!("diff <(head -5 {FILE}) <(cat {FILE}@/lines:1-5)")); }

/// T-105: `file@/lines:M` — single line read matches `sed -n 'Mp'`.
#[rstest]
fn t_105_lines_single(mount: NyneMount) { mount.sh_ok(&format!("diff <(sed -n '3p' {FILE}) <(cat {FILE}@/lines:3)")); }

/// T-106: `file@/symbols/imports.rs` — import block contains `use` statements.
#[rstest]
fn t_106_imports_block(mount: NyneMount) {
    assert_contains(&mount.read(&format!("{FILE}@/symbols/imports.rs")), "use ");
}

/// T-107: `file@/symbols/by-kind/` — kind-filtered directories are listed.
#[rstest]
fn t_107_by_kind_directories(mount: NyneMount) {
    assert_contains_any(&mount.sh_ok(&format!("ls {FILE}@/symbols/by-kind/")), &[
        "fn", "struct", "enum", "module", "impl", "const",
    ]);
}
