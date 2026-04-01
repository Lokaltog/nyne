//! Category 20 — Edge cases and boundary tests (T-1900..T-1902, read-only subset).
//!
//! T-1903 (write-then-read consistency) and T-1904 (git notes write) are
//! mutating and belong to a later phase.

use nyne_integration_tests::targets::rust::{FILE, IMPL, SYMBOL};
use nyne_integration_tests::{NyneMount, assert_ok, mount};
use rstest::rstest;

/// T-1900: Symbol without a docstring — read returns empty content or ENOENT.
///
/// `Command~Impl` is a bare impl block with no doc comment; its
/// `docstring.txt` should resolve to empty or not exist. Either is acceptable.
#[rstest]
fn t_1900_empty_docstring(mount: NyneMount) {
    let out = mount.sh(&format!("cat {FILE}@/symbols/{IMPL}@/docstring.txt"));
    // Two valid outcomes: success with empty content, or a failed cat.
    if out.is_ok() {
        assert!(out.stdout.trim().is_empty(), "expected empty docstring");
    }
}

/// T-1901: Large symbol body — read completes and returns substantial content.
///
/// `build_fuse_session` in `mount.rs` is one of the largest functions in the
/// project (~1k tokens) and exercises the full splice engine.
#[rstest]
fn t_1901_large_symbol_body(mount: NyneMount) {
    let body = mount.read("nyne/src/cli/mount.rs@/symbols/build_fuse_session@/body.rs");
    assert!(
        body.len() >= 1000,
        "expected substantial body content, got {} bytes",
        body.len()
    );
}

/// T-1902: Concurrent reads — all clients observe identical content.
#[rstest]
fn t_1902_concurrent_reads(mount: NyneMount) {
    let out = mount.sh(&format!(
        "for i in 1 2 3 4 5; do \
            cat {FILE}@/symbols/{SYMBOL}@/body.rs > /tmp/nyne_concurrent_$i & \
         done; wait; \
         first=$(md5sum /tmp/nyne_concurrent_1 | awk '{{print $1}}'); \
         for i in 2 3 4 5; do \
            m=$(md5sum /tmp/nyne_concurrent_$i | awk '{{print $1}}'); \
            [ \"$m\" = \"$first\" ] || {{ echo MISMATCH:$i; exit 1; }}; \
         done; \
         rm -f /tmp/nyne_concurrent_*"
    ));
    assert_ok(&out);
    assert!(
        !out.stdout.contains("MISMATCH"),
        "concurrent reads diverged: {}",
        out.stdout
    );
}
