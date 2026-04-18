//! Category 3 — Symbol read features (T-200..T-207).
//!
//! Validates shorthand/explicit body reads, signature/docstring/decorator
//! slices, the per-symbol OVERVIEW, nested child access, and the symbol
//! directory listing.

use nyne_integration_tests::targets::rust::{FILE, IMPL, NESTED, SYMBOL};
use nyne_integration_tests::{NyneMount, assert_contains, assert_contains_any, assert_ok, mount};
use rstest::rstest;

/// T-200: `symbols/Foo.rs` — shorthand body read returns non-empty content.
#[rstest]
fn t_200_shorthand_body(mount: NyneMount) {
    let body = mount.read(&format!("{FILE}@/symbols/{SYMBOL}.rs"));
    assert!(!body.trim().is_empty(), "shorthand body should be non-empty");
    assert_contains(&body, "struct");
}

/// T-201: `symbols/Foo@/body.rs` — explicit body equals shorthand read.
#[rstest]
fn t_201_explicit_body_equals_shorthand(mount: NyneMount) {
    assert_ok(&mount.sh(&format!(
        "diff <(cat {FILE}@/symbols/{SYMBOL}.rs) \
              <(cat {FILE}@/symbols/{SYMBOL}@/body.rs)"
    )));
}

/// T-202: `symbols/Foo@/signature.rs` — declaration only, small.
#[rstest]
fn t_202_signature_is_declaration(mount: NyneMount) {
    let sig = mount.read(&format!("{FILE}@/symbols/{SYMBOL}@/signature.rs"));
    assert_contains(&sig, SYMBOL);
    assert!(sig.lines().count() <= 5, "signature should be small (≤5 lines):\n{sig}");
}

/// T-203: `symbols/Foo@/docstring.txt` — docstring with comment markers stripped.
#[rstest]
fn t_203_docstring_stripped(mount: NyneMount) {
    let doc = mount.read(&format!("{FILE}@/symbols/{SYMBOL}@/docstring.txt"));
    assert!(!doc.trim().is_empty(), "Cli has a docstring");
    assert!(!doc.contains("///"), "/// markers should be stripped on read");
    assert!(!doc.contains("//!"), "//! markers should be stripped on read");
}

/// T-204: `symbols/Foo@/decorators.rs` — Cli's derive/command attributes.
#[rstest]
fn t_204_decorators(mount: NyneMount) {
    assert_contains_any(&mount.read(&format!("{FILE}@/symbols/{SYMBOL}@/decorators.rs")), &[
        "#[derive",
        "#[command",
    ]);
}

/// T-205: `symbols/Parent@/OVERVIEW.md` — lists nested child symbols.
#[rstest]
fn t_205_parent_overview(mount: NyneMount) {
    assert_contains(&mount.read(&format!("{FILE}@/symbols/{IMPL}@/OVERVIEW.md")), NESTED);
}

/// T-206: `symbols/Parent@/Child@/body.rs` — nested child read.
#[rstest]
fn t_206_nested_child_body(mount: NyneMount) {
    let body = mount.read(&format!("{FILE}@/symbols/{IMPL}@/{NESTED}@/body.rs"));
    assert!(!body.trim().is_empty(), "nested child body should be non-empty");
    assert_contains(&body, "fn ");
}

/// T-207: `ls symbols/Foo@/` — directory lists standard companion entries.
///
/// `edit/` is intentionally hidden from `ls` (write-only staging surface,
/// reachable only via direct path access) and is not asserted here.
#[rstest]
fn t_207_symbol_directory_listing(mount: NyneMount) {
    let out = mount.sh(&format!("ls {FILE}@/symbols/{SYMBOL}@/"));
    assert_ok(&out);
    for entry in &[
        "body.rs",
        "signature.rs",
        "docstring.txt",
        "CALLERS.md",
        "REFERENCES.md",
        "actions",
    ] {
        assert_contains(&out.stdout, entry);
    }
}
