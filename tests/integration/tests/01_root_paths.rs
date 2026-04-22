//! Category 1 — Root `@/` path features (T-003..T-010).
//!
//! Validates VFS nodes served from the mount root: git state, TODO aggregation,
//! batch edit staging, and the Claude Code system prompt.

use nyne_integration_tests::{NyneMount, assert_contains, assert_contains_any, assert_ok, mount, targets};
use rstest::rstest;

/// T-003: `@/git/STATUS.md` — repository status contains a branch name.
#[rstest]
fn t_003_git_status(mount: NyneMount) {
    mount.cat_contains_any("@/git/STATUS.md", &["main", "master", "refactor/", "branch"]);
}

/// T-004: `@/git/branches/` — branch listing is non-empty.
#[rstest]
fn t_004_git_branches(mount: NyneMount) {
    assert!(
        !mount.sh_ok("ls @/git/branches/").trim().is_empty(),
        "expected at least one branch entry",
    );
}

/// T-005: `@/git/tags/` — tag listing directory exists (may be empty).
#[rstest]
fn t_005_git_tags(mount: NyneMount) { mount.sh_ok("ls @/git/tags/"); }

/// T-006: `@/todo/OVERVIEW.md` — TODO aggregation index references at least one tag.
#[rstest]
fn t_006_todo_overview(mount: NyneMount) {
    mount.cat_contains_any("@/todo/OVERVIEW.md", &["TODO", "FIXME", "HACK", "SAFETY", "XXX"]);
}

/// T-007: `@/todo/<TAG>.md` — at least one per-tag aggregation file is readable.
#[rstest]
fn t_007_todo_tag_files(mount: NyneMount) {
    // Any of the common tags should exist and be readable.
    assert_contains(
        &mount.sh_ok(
            "for t in TODO FIXME HACK SAFETY XXX; do \
                if [ -f \"@/todo/${t}.md\" ]; then echo FOUND:$t; cat \"@/todo/${t}.md\"; break; fi; \
             done",
        ),
        "FOUND:",
    );
}

/// T-008: `@/todo/<TAG>/` — per-tag symlink directory has resolvable entries.
#[rstest]
fn t_008_todo_tag_symlinks(mount: NyneMount) {
    // Find a tag directory that exists and has at least one entry.
    assert_contains(
        &mount.sh_ok(
            "for t in TODO FIXME HACK SAFETY XXX; do \
                if [ -d \"@/todo/${t}\" ]; then \
                    first=$(ls \"@/todo/${t}/\" | head -1); \
                    if [ -n \"$first\" ]; then \
                        echo ENTRY:$first; \
                        readlink \"@/todo/${t}/${first}\"; \
                        break; \
                    fi; \
                fi; \
             done",
        ),
        "ENTRY:",
    );
}

/// T-009: `@/edit/staged.diff` — root batch-edit staging preview is readable.
#[rstest]
fn t_009_edit_staged_diff(mount: NyneMount) {
    // Either empty-state message or a valid diff.
    mount.cat_contains_any("@/edit/staged.diff", &["No changes", "No staged", "---", "diff --git"]);
}

/// T-010: `@/agents/claude-code/system-prompts/default.md` — Claude system prompt is non-empty.
#[rstest]
fn t_010_claude_system_prompt(mount: NyneMount) {
    let stdout = mount.read("@/agents/claude-code/system-prompts/default.md");
    assert!(!stdout.trim().is_empty(), "expected non-empty system prompt");
    assert_contains_any(&stdout, &["nyne", "VFS", "OVERVIEW", "@/", "symbol"]);
}

/// T-011: writing to an `edit/insert-after` staging endpoint stages the op
/// and surfaces it in `staged.diff`.
///
/// Regression guard for the `on_create`-based staging pipeline: the kernel
/// issues `create(2)` because the endpoint is hidden from lookup, so the
/// FUSE bridge's `is_writable_dir` pre-check and the post-callback
/// `next.run` propagation must both permit the create path without
/// tripping on the terminal `fs` provider's companion-mutation rejection.
#[rstest]
#[serial_test::serial]
fn t_011_edit_insert_after_stages(mount: NyneMount) {
    let _guard = mount.cleanup_guard();

    let edit_dir = format!("{}@/symbols/{}@/edit", targets::rust::FILE, targets::rust::SYMBOL);

    mount.sh_ok(&format!(
        "printf '/// staged-marker-t011\\nfn __t011() {{}}\\n' > {edit_dir}/insert-after"
    ));

    assert_contains(
        &mount.sh_ok(&format!("cat {edit_dir}/staged.diff")),
        "staged-marker-t011",
    );

    // Drain without applying so the source file is untouched — the scoped
    // `> staged.diff` write clears only this file's ops.
    mount.sh_ok(&format!("> {edit_dir}/staged.diff"));
}
/// T-012: `statx` on an `edit/insert-after` staging endpoint succeeds
/// inside the [`STAGE_ENDPOINT_TTL`] window after the write.
///
/// Regression guard for the Claude Code post-write `statx` failure: CC
/// issues `creat()` → `write()` → `statx(AT_STATX_SYNC_AS_STAT)` and
/// the third call must succeed even though the sink endpoint is hidden
/// from `LOOKUP`. The fix relies on `CachePolicy::Ttl` being honored
/// in the kernel `entry_valid`/`attr_valid` window of the CREATE reply.
///
/// Implementation note: GNU coreutils `stat` calls `statx` with
/// `AT_STATX_SYNC_AS_STAT` (the same flags CC uses), so a single
/// shell invocation reproduces the bug exactly.
#[rstest]
#[serial_test::serial]
fn t_012_edit_insert_after_statx_after_write(mount: NyneMount) {
    let _guard = mount.cleanup_guard();

    let edit_dir = format!("{}@/symbols/{}@/edit", targets::rust::FILE, targets::rust::SYMBOL);
    let endpoint = format!("{edit_dir}/insert-after");

    // GNU coreutils `stat` calls `statx(AT_STATX_SYNC_AS_STAT)` —
    // the same flags Claude Code's post-write verification uses.
    // The bug surfaces if the kernel re-issues LOOKUP for the
    // post-RELEASE statx (sink endpoints return ENOENT on lookup).
    assert_contains(
        &mount.sh_ok(&format!(
            "printf 'fn __t012() {{}}\\n' > {endpoint} && stat -c '%n %s' {endpoint}"
        )),
        "insert-after",
    );

    // Clear staged ops so the source file is untouched.
    mount.sh_ok(&format!("> {edit_dir}/staged.diff"));
}
