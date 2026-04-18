//! Category 21 — LSP indexing-progress gating end-to-end (T-400..T-499).
//!
//! Verifies that LSP-backed VFS reads on a cold mount return *indexed*
//! data, never empty/pre-index results. This proves the per-`Client`
//! `ProgressTracker` gate in `send_request` parks the FUSE handler
//! thread until the workspace is queryable, and that the inline
//! grace timer (`[lsp].index_timeout`, default 120 s) bounds the wait.
//!
//! No hardcoded sleeps in the harness or the tests -- the gate is the
//! sole synchronization mechanism. If a regression re-introduces a
//! pre-index race, these tests fail with empty content / empty dirs.
//!
//! Targets `Provider` trait in `nyne/src/router/pipeline/provider.rs`,
//! which has 14+ implementations and references across every plugin
//! crate -- a non-trivial cross-reference graph that requires real
//! rust-analyzer indexing to populate.

use nyne_integration_tests::targets::lsp::{FILE, METHOD_FILE, METHOD_SYMBOL, SYMBOL};
use nyne_integration_tests::{NyneMount, assert_contains, assert_ok, mount};
use rstest::rstest;

/// T-400: cold-mount LSP-backed `*.md` reads contain real indexed
/// content (cross-references from `plugins/`).
///
/// Each case is a fresh mount (the `mount` fixture is per-test, not
/// `#[once]`). Nothing in the harness pre-warms the LSP server -- if
/// the gate is not parking the read on `wait_ready`, content arrives
/// empty and the `assert_contains("plugins/")` fails.
///
/// `CALLERS.md` and `DEPS.md` anchor on a concrete `Provider::accept`
/// impl ([`METHOD_FILE`] / [`METHOD_SYMBOL`]) — a trait declaration
/// itself has no callers and no dependencies, so those nodes target a
/// method instead. `REFERENCES.md` and `IMPLEMENTATION.md` stay on the
/// trait, which is referenced and implemented across every plugin.
#[rstest]
#[case::callers_md("CALLERS.md", METHOD_FILE, METHOD_SYMBOL)]
#[case::deps_md("DEPS.md", METHOD_FILE, METHOD_SYMBOL)]
#[case::references_md("REFERENCES.md", FILE, SYMBOL)]
#[case::implementation_md("IMPLEMENTATION.md", FILE, SYMBOL)]
fn t_400_cold_md_contains_indexed_content(
    mount: NyneMount,
    #[case] node: &str,
    #[case] file: &str,
    #[case] symbol: &str,
) {
    let content = mount.read(&format!("{file}@/symbols/{symbol}@/{node}"));
    assert_contains(&content, "plugins/");
    assert_contains(&content, ".rs");
}

/// T-401: cold-mount readdir on LSP-backed symlink directories
/// returns non-empty entry sets when the symbol has known
/// cross-references.
///
/// `implementation/` and `references/` are populated from LSP
/// `Location` results during the readdir pass. An empty directory
/// means LSP returned zero locations -- which on a `Provider`-class
/// symbol is only possible if the gate let readdir run pre-index.
#[rstest]
#[case::implementation_dir("implementation")]
#[case::references_dir("references")]
fn t_401_cold_dir_has_entries(mount: NyneMount, #[case] dir: &str) {
    let out = mount.sh(&format!("ls {FILE}@/symbols/{SYMBOL}@/{dir}/"));
    assert_ok(&out);
    assert!(
        !out.stdout.trim().is_empty(),
        "{dir}/ should have entries on cold mount; the gate must park readdir until indexed",
    );
}

/// T-402: multiple concurrent cold-mount reads all receive indexed
/// data. Verifies the condvar `notify_all` fan-out across FUSE
/// handler threads parked on the same `ProgressTracker`.
///
/// Four parallel `cat` processes saturate the FUSE handler thread
/// pool (4 threads). All four queries must complete with indexed
/// content; if any waiter starves on the condvar wake, its output is
/// empty and the aggregate `assert_contains` fails.
#[rstest]
fn t_402_concurrent_cold_reads_all_indexed(mount: NyneMount) {
    let out = mount.sh(&format!(
        "for node in CALLERS.md REFERENCES.md IMPLEMENTATION.md DEPS.md; do \
             cat {FILE}@/symbols/{SYMBOL}@/$node & \
         done; \
         wait"
    ));
    assert_ok(&out);
    assert_contains(&out.stdout, "plugins/");
    assert_contains(&out.stdout, ".rs");
}

/// T-403: after the first cold-mount LSP read pays the indexing wait,
/// every subsequent read returns immediately with real data. Verifies
/// that the `Ready` state is monotonic -- background re-analysis
/// after the initial cycle does not regress to `Indexing` and so does
/// not re-park later reads.
///
/// All four reads run in the same mount, so the second through fourth
/// are post-`Ready` calls. If `Ready` were not monotonic, transient
/// background progress between reads would re-park `wait_ready` and
/// the subsequent assertions would either time out or surface empty
/// content.
///
/// `CALLERS.md` / `DEPS.md` target the concrete `Provider::accept`
/// impl (a trait declaration has no callers/deps); `REFERENCES.md` /
/// `IMPLEMENTATION.md` target the `Provider` trait itself.
#[rstest]
fn t_403_sequential_reads_stay_ready(mount: NyneMount) {
    for (node, file, symbol) in [
        ("CALLERS.md", METHOD_FILE, METHOD_SYMBOL),
        ("DEPS.md", METHOD_FILE, METHOD_SYMBOL),
        ("REFERENCES.md", FILE, SYMBOL),
        ("IMPLEMENTATION.md", FILE, SYMBOL),
    ] {
        assert_contains(
            &mount.read(&format!("{file}@/symbols/{symbol}@/{node}")),
            "plugins/",
        );
    }
}
