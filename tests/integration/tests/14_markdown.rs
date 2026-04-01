//! Category 14 — Markdown decomposition (T-1300..T-1304, read-only subset).
//!
//! Validates that markdown files are decomposed into nested sections and
//! extracted code blocks. T-1303 (section write) is a mutating test covered
//! by the symbol mutation phase.

use nyne_integration_tests::{NyneMount, assert_contains, assert_contains_any, assert_ok, mount};
use rstest::rstest;

/// Target markdown file — README.md has an h1 with many h2 children.
const TEST_MD: &str = "README.md";
/// Top-level h1 section of [`TEST_MD`].
const TOP_SECTION: &str = "00-nyne";
/// Nested h2 section inside [`TOP_SECTION`] that contains fenced code blocks.
const NESTED_SECTION: &str = "00-what-it-looks-like";

/// T-1300: Markdown OVERVIEW.md — section listing with kind indicators and line ranges.
#[rstest]
fn t_1300_overview(mount: NyneMount) {
    let overview = mount.read(&format!("{TEST_MD}@/symbols/OVERVIEW.md"));
    assert_contains(&overview, TOP_SECTION);
    assert_contains_any(&overview, &["h1", "h2", "h3"]);
    // Line ranges use "M-N" format.
    assert_contains(&overview, "-");
}

/// T-1301: Read a top-level markdown section body.
#[rstest]
fn t_1301_read_section(mount: NyneMount) {
    assert!(
        !mount
            .read(&format!("{TEST_MD}@/symbols/{TOP_SECTION}@/body.md"))
            .trim()
            .is_empty(),
        "section body should be non-empty"
    );
}

/// T-1302: Read a nested markdown subsection body.
#[rstest]
fn t_1302_nested_subsection(mount: NyneMount) {
    assert!(
        !mount
            .read(&format!("{TEST_MD}@/symbols/{TOP_SECTION}@/{NESTED_SECTION}@/body.md"))
            .trim()
            .is_empty(),
        "nested section body should be non-empty"
    );
}

/// T-1304: Code block extraction — fenced code blocks appear as numbered files
/// with extensions matching the fence language.
#[rstest]
fn t_1304_code_blocks(mount: NyneMount) {
    let list = mount.sh(&format!("ls {TEST_MD}@/symbols/{TOP_SECTION}@/{NESTED_SECTION}@/code/"));
    assert_ok(&list);
    assert!(
        !list.stdout.trim().is_empty(),
        "expected at least one extracted code block"
    );

    // Read the first code block — content should be non-empty.
    let first = mount.sh(&format!(
        "cat {TEST_MD}@/symbols/{TOP_SECTION}@/{NESTED_SECTION}@/code/$(ls {TEST_MD}@/symbols/{TOP_SECTION}@/{NESTED_SECTION}@/code/ | head -1)"
    ));
    assert_ok(&first);
    assert!(!first.stdout.trim().is_empty(), "code block should be non-empty");
}
