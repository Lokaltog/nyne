//! Category 4 — LSP features on symbols (T-300..T-317).
//!
//! Validates LSP-powered intelligence via rust-analyzer: call hierarchy,
//! dependencies, references, declaration/definition/implementation, type
//! definition, hover docs, inlay hints, code actions, and rename preview.
//!
//! Targets `Provider` trait in `nyne/src/router/provider.rs` — a widely
//! implemented and referenced trait that populates all LSP views.

use nyne_integration_tests::targets::lsp::{FILE, SYMBOL};
use nyne_integration_tests::{NyneMount, assert_contains, assert_contains_any, mount};
use rstest::rstest;

/// T-300: `CALLERS.md` — incoming call hierarchy node is readable.
#[rstest]
fn t_300_callers_md(mount: NyneMount) {
    assert_contains_any(&mount.read(&format!("{FILE}@/symbols/{SYMBOL}@/CALLERS.md")), &[
        "Incoming Calls",
        "Callers",
        "No incoming calls",
    ]);
}

/// T-301: `callers/` — caller symlink directory exists.
#[rstest]
fn t_301_callers_dir(mount: NyneMount) { mount.sh_ok(&format!("ls {FILE}@/symbols/{SYMBOL}@/callers/")); }

/// T-302: `DEPS.md` — outgoing dependencies node is readable.
#[rstest]
fn t_302_deps_md(mount: NyneMount) {
    assert_contains_any(&mount.read(&format!("{FILE}@/symbols/{SYMBOL}@/DEPS.md")), &[
        "Dependencies",
        "No dependencies",
    ]);
}

/// T-303: `deps/` — dependency symlink directory exists.
#[rstest]
fn t_303_deps_dir(mount: NyneMount) { mount.sh_ok(&format!("ls {FILE}@/symbols/{SYMBOL}@/deps/")); }

/// T-304: `REFERENCES.md` — lists file paths and line numbers.
#[rstest]
fn t_304_references_md(mount: NyneMount) {
    let refs = mount.read(&format!("{FILE}@/symbols/{SYMBOL}@/REFERENCES.md"));
    assert_contains_any(&refs, &["References", "File"]);
    // Provider trait is referenced across many plugin crates.
    assert_contains(&refs, "plugins/");
    assert_contains(&refs, ".rs");
}

/// T-305: `references/` — reference symlink directory has entries when
/// REFERENCES.md is non-empty.
#[rstest]
fn t_305_references_dir(mount: NyneMount) {
    assert!(
        !mount
            .sh_ok(&format!("ls {FILE}@/symbols/{SYMBOL}@/references/"))
            .trim()
            .is_empty(),
        "Provider trait has >70 references; references/ should have entries"
    );
}

/// T-306: `DECLARATION.md` — contains a file path and line number.
#[rstest]
fn t_306_declaration_md(mount: NyneMount) {
    assert_contains(&mount.read(&format!("{FILE}@/symbols/{SYMBOL}@/DECLARATION.md")), FILE);
}

/// T-307: `declaration/` — declaration symlink directory exists.
#[rstest]
fn t_307_declaration_dir(mount: NyneMount) { mount.sh_ok(&format!("ls {FILE}@/symbols/{SYMBOL}@/declaration/")); }

/// T-308: `DEFINITION.md` — contains a file path and line number.
#[rstest]
fn t_308_definition_md(mount: NyneMount) {
    assert_contains(&mount.read(&format!("{FILE}@/symbols/{SYMBOL}@/DEFINITION.md")), FILE);
}

/// T-309: `definition/` — definition symlink directory exists.
#[rstest]
fn t_309_definition_dir(mount: NyneMount) { mount.sh_ok(&format!("ls {FILE}@/symbols/{SYMBOL}@/definition/")); }

/// T-310: `IMPLEMENTATION.md` — lists concrete impl sites.
#[rstest]
fn t_310_implementation_md(mount: NyneMount) {
    let impls = mount.read(&format!("{FILE}@/symbols/{SYMBOL}@/IMPLEMENTATION.md"));
    // Provider trait has 14 implementations across plugin crates.
    assert_contains(&impls, "plugins/");
    assert_contains(&impls, ".rs");
}

/// T-311: `implementation/` — implementation symlink directory has entries.
#[rstest]
fn t_311_implementation_dir(mount: NyneMount) {
    assert!(
        !mount
            .sh_ok(&format!("ls {FILE}@/symbols/{SYMBOL}@/implementation/"))
            .trim()
            .is_empty(),
        "Provider has 14 implementations; implementation/ should have entries"
    );
}

/// T-312: `TYPE-DEFINITION.md` — node is readable.
#[rstest]
fn t_312_type_definition_md(mount: NyneMount) {
    mount.sh_ok(&format!("cat {FILE}@/symbols/{SYMBOL}@/TYPE-DEFINITION.md"));
}

/// T-313: `type-definition/` — type definition symlink directory exists.
#[rstest]
fn t_313_type_definition_dir(mount: NyneMount) {
    mount.sh_ok(&format!("ls {FILE}@/symbols/{SYMBOL}@/type-definition/"));
}

/// T-314: `DOC.md` — hover documentation contains symbol name.
#[rstest]
fn t_314_doc_md(mount: NyneMount) {
    let doc = mount.read(&format!("{FILE}@/symbols/{SYMBOL}@/DOC.md"));
    assert!(!doc.trim().is_empty(), "DOC.md should be non-empty");
    assert_contains(&doc, SYMBOL);
}

/// T-315: `HINTS.md` — inlay hints node is readable.
#[rstest]
fn t_315_hints_md(mount: NyneMount) {
    assert_contains_any(&mount.read(&format!("{FILE}@/symbols/{SYMBOL}@/HINTS.md")), &[
        "Hints",
        "hints",
        "No inlay hints",
    ]);
}

/// T-316: `actions/` — at least one code action `.diff` file exists and
/// contains unified diff markers.
#[rstest]
fn t_316_actions_dir(mount: NyneMount) {
    // Read first action via shell chain: list → head → cat.
    let diff = mount.sh_ok(&format!(
        "cat {FILE}@/symbols/{SYMBOL}@/actions/$(ls {FILE}@/symbols/{SYMBOL}@/actions/ | head -1)"
    ));
    for marker in &["---", "+++", "@@"] {
        assert_contains(&diff, marker);
    }
}

/// T-317: `rename/<new>.diff` — rename preview node is readable and produces
/// a unified diff. Multi-file scope is tested by Category 18 (T-1700).
#[rstest]
fn t_317_rename_preview_readable(mount: NyneMount) {
    let diff = mount.read(&format!("{FILE}@/symbols/{SYMBOL}@/rename/RenamedProvider.diff"));
    for marker in &["---", "+++", "@@"] {
        assert_contains(&diff, marker);
    }
}
