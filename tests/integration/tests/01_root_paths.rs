//! Category 1 — Root `@/` path features (T-003..T-010).
//!
//! Validates VFS nodes served from the mount root: git state, TODO aggregation,
//! batch edit staging, and the Claude Code system prompt.

use nyne_integration_tests::{NyneMount, assert_contains, assert_contains_any, assert_ok, mount, targets};
use rstest::rstest;

/// T-003: `@/git/STATUS.md` — repository status contains a branch name.
#[rstest]
fn t_003_git_status(mount: NyneMount) {
    let out = mount.sh("cat @/git/STATUS.md");
    assert_ok(&out);
    assert_contains_any(&out.stdout, &["main", "master", "refactor/", "branch"]);
}

/// T-004: `@/git/branches/` — branch listing is non-empty.
#[rstest]
fn t_004_git_branches(mount: NyneMount) {
    let out = mount.sh("ls @/git/branches/");
    assert_ok(&out);
    assert!(!out.stdout.trim().is_empty(), "expected at least one branch entry");
}

/// T-005: `@/git/tags/` — tag listing directory exists (may be empty).
#[rstest]
fn t_005_git_tags(mount: NyneMount) { assert_ok(&mount.sh("ls @/git/tags/")); }

/// T-006: `@/todo/OVERVIEW.md` — TODO aggregation index references at least one tag.
#[rstest]
fn t_006_todo_overview(mount: NyneMount) {
    let out = mount.sh("cat @/todo/OVERVIEW.md");
    assert_ok(&out);
    assert_contains_any(&out.stdout, &["TODO", "FIXME", "HACK", "SAFETY", "XXX"]);
}

/// T-007: `@/todo/<TAG>.md` — at least one per-tag aggregation file is readable.
#[rstest]
fn t_007_todo_tag_files(mount: NyneMount) {
    // Any of the common tags should exist and be readable.
    let out = mount.sh("for t in TODO FIXME HACK SAFETY XXX; do \
            if [ -f \"@/todo/${t}.md\" ]; then echo FOUND:$t; cat \"@/todo/${t}.md\"; break; fi; \
         done");
    assert_ok(&out);
    assert_contains(&out.stdout, "FOUND:");
}

/// T-008: `@/todo/<TAG>/` — per-tag symlink directory has resolvable entries.
#[rstest]
fn t_008_todo_tag_symlinks(mount: NyneMount) {
    // Find a tag directory that exists and has at least one entry.
    let out = mount.sh("for t in TODO FIXME HACK SAFETY XXX; do \
            if [ -d \"@/todo/${t}\" ]; then \
                first=$(ls \"@/todo/${t}/\" | head -1); \
                if [ -n \"$first\" ]; then \
                    echo ENTRY:$first; \
                    readlink \"@/todo/${t}/${first}\"; \
                    break; \
                fi; \
            fi; \
         done");
    assert_ok(&out);
    assert_contains(&out.stdout, "ENTRY:");
}

/// T-009: `@/edit/staged.diff` — root batch-edit staging preview is readable.
#[rstest]
fn t_009_edit_staged_diff(mount: NyneMount) {
    let out = mount.sh("cat @/edit/staged.diff");
    assert_ok(&out);
    // Either empty-state message or a valid diff.
    assert_contains_any(&out.stdout, &["No changes", "No staged", "---", "diff --git"]);
}

/// T-010: `@/agents/claude-code/system-prompts/default.md` — Claude system prompt is non-empty.
#[rstest]
fn t_010_claude_system_prompt(mount: NyneMount) {
    let out = mount.sh("cat @/agents/claude-code/system-prompts/default.md");
    assert_ok(&out);
    assert!(!out.stdout.trim().is_empty(), "expected non-empty system prompt");
    assert_contains_any(&out.stdout, &["nyne", "VFS", "OVERVIEW", "@/", "symbol"]);
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

    assert_ok(&mount.sh(&format!(
        "printf '/// staged-marker-t011\\nfn __t011() {{}}\\n' > {edit_dir}/insert-after"
    )));

    let preview = mount.sh(&format!("cat {edit_dir}/staged.diff"));
    assert_ok(&preview);
    assert_contains(&preview.stdout, "staged-marker-t011");

    // Drain without applying so the source file is untouched — the scoped
    // `> staged.diff` write clears only this file's ops.
    assert_ok(&mount.sh(&format!("> {edit_dir}/staged.diff")));
}
