use super::*;

/// Tests that `hunk_overlaps_range` correctly detects overlap between blame hunks and line ranges.
mod hunk_overlap_tests {
    use nyne::types::SymbolLineRange;
    use rstest::rstest;

    use super::*;

    /// Build a test `BlameHunk` from a line spec like `"5"` or `"5-10"`.
    fn hunk(lines: &str) -> history::BlameHunk {
        let (start_line, end_line) = if let Some((s, e)) = lines.split_once('-') {
            (s.parse().unwrap(), e.parse().unwrap())
        } else {
            let line: usize = lines.parse().unwrap();
            (line, line)
        };
        history::BlameHunk {
            start_line,
            end_line,
            commit: crate::commit::CommitInfo {
                hash: "abc1234".into(),
                author: "dev".into(),
                date: "2024-01-15".into(),
                message: "test".into(),
                epoch_secs: 0,
            },
        }
    }

    /// Verifies hunk overlap detection against a fixed 10-20 range with various hunk positions.
    #[rstest]
    #[case::single_line_inside("15", true)]
    #[case::range_fully_inside("12-18", true)]
    #[case::overlaps_start("5-12", true)]
    #[case::overlaps_end("18-25", true)]
    #[case::before_range("1-9", false)]
    #[case::after_range("21-30", false)]
    #[case::exact_boundaries("10-20", true)]
    #[case::at_start_boundary("10", true)]
    #[case::at_end_boundary("20", true)]
    #[case::just_before_start("9", false)]
    #[case::just_after_end("21", false)]
    fn hunk_overlap(#[case] lines: &str, #[case] expected: bool) {
        let range = SymbolLineRange { start: 10, end: 20 };
        assert_eq!(
            views::hunk_overlaps_range(&hunk(lines), &range),
            expected,
            "hunk {lines} vs range 10-20"
        );
    }
}

/// Tests for `SymbolLineRange` construction and formatting.
mod symbol_line_range_tests {
    use nyne::types::SymbolLineRange;

    /// Tests that `from_zero_based` converts a 0-based range to 1-based inclusive.
    #[test]
    fn from_zero_based() {
        let range = SymbolLineRange::from_zero_based(&(5..10));
        assert_eq!(range.start, 6);
        assert_eq!(range.end, 10);
    }

    /// Tests that a single-element 0-based range converts to a single 1-based line.
    #[test]
    fn from_zero_based_single_line() {
        let range = SymbolLineRange::from_zero_based(&(0..1));
        assert_eq!(range.start, 1);
        assert_eq!(range.end, 1);
    }

    /// Tests byte-range conversion for a single-line source file.
    #[test]
    fn from_byte_range_single_line_file() {
        let source = "fn foo() {}\n";
        // full_span covers the entire line (bytes 0..11, before the \n)
        let range = SymbolLineRange::from_byte_range(source, &(0..11));
        assert_eq!(range, SymbolLineRange { start: 1, end: 1 });
    }

    /// Tests byte-range conversion spanning multiple lines.
    #[test]
    fn from_byte_range_multi_line() {
        let source = "fn foo() {\n    42\n}\n";
        // full_span covers bytes 0..18 (up to and including `}`)
        let range = SymbolLineRange::from_byte_range(source, &(0..18));
        assert_eq!(range, SymbolLineRange { start: 1, end: 3 });
    }

    /// Tests byte-range conversion when the span starts after preceding content.
    #[test]
    fn from_byte_range_with_preceding_content() {
        let source = "use std::io;\n\nfn bar() {\n    1\n}\n";
        // `fn bar` starts at byte 14 (after "use std::io;\n\n"), `}` at byte 31
        let range = SymbolLineRange::from_byte_range(source, &(14..31));
        assert_eq!(range, SymbolLineRange { start: 3, end: 5 });
    }

    /// Tests byte-range conversion for a decorator-only span.
    #[test]
    fn from_byte_range_decorator_span() {
        // Decorator on line 1, function on lines 2-4.
        let source = "#[derive(Debug)]\npub struct Foo;\n";
        // Decorator range: bytes 0..16 (`#[derive(Debug)]`)
        let range = SymbolLineRange::from_byte_range(source, &(0..16));
        assert_eq!(range, SymbolLineRange { start: 1, end: 1 });
    }

    /// Verifies `to_string()` produces `"lines:5-10"` for a multi-line range.
    #[test]
    fn display_range() {
        let range = SymbolLineRange { start: 5, end: 10 };
        assert_eq!(range.to_string(), "lines:5-10");
    }

    /// Verifies `to_string()` produces `"lines:3"` for a single-line range.
    #[test]
    fn display_single() {
        let range = SymbolLineRange { start: 3, end: 3 };
        assert_eq!(range.to_string(), "lines:3");
    }
}

/// Tests for `history_filename` formatting (sequence number, date, hash, kebab message).
mod history_filename_tests {
    use super::*;

    /// Build a test `HistoryEntry` with the given commit message.
    fn entry(message: &str) -> history::HistoryEntry {
        history::HistoryEntry {
            oid: git2::Oid::zero(),
            hash: "abc1234".into(),
            author: "dev".into(),
            date: "2024-01-15".into(),
            message: message.into(),
            epoch_secs: 1_705_276_800,
        }
    }

    /// Verifies basic filename format with sequence, date, hash, and kebab message.
    #[test]
    fn basic() {
        let e = entry("fix the bug");
        assert_eq!(
            views::history_filename(0, &e, "rs"),
            "001_2024-01-15_abc1234_fix-the-bug.rs"
        );
    }

    /// Verifies that the sequence number is 1-based and zero-padded.
    #[test]
    fn sequence_number() {
        let e = entry("second commit");
        assert_eq!(
            views::history_filename(4, &e, "rs"),
            "005_2024-01-15_abc1234_second-commit.rs"
        );
    }

    /// Verifies filename omits the trailing `.ext` when extension is empty.
    #[test]
    fn no_extension() {
        let e = entry("initial commit");
        assert_eq!(
            views::history_filename(0, &e, ""),
            "001_2024-01-15_abc1234_initial-commit"
        );
    }

    /// Verifies that special characters in commit messages are sanitized to kebab-case.
    #[test]
    fn special_chars_in_message() {
        let e = entry("feat(scope): add thing!");
        assert_eq!(
            views::history_filename(0, &e, "py"),
            "001_2024-01-15_abc1234_feat-scope-add-thing.py"
        );
    }

    /// Verifies that long commit messages are truncated to at most 50 characters.
    #[test]
    fn long_message_truncated() {
        let e = entry("this is a very long commit message that exceeds the fifty character limit");
        let name = views::history_filename(0, &e, "rs");
        assert!(name.starts_with("001_2024-01-15_abc1234_"));
        assert!(name.ends_with(".rs"));
        let kebab = name
            .strip_prefix("001_2024-01-15_abc1234_")
            .unwrap()
            .strip_suffix(".rs")
            .unwrap();
        assert!(kebab.len() <= 50);
    }
}

/// Tests for sliced blame and log views with real git repos.
mod sliced_content_tests {
    use std::sync::Arc;

    use nyne::templates::{HandleBuilder, TemplateHandle};
    use nyne::types::slice::SliceSpec;

    use super::*;

    /// Template handles for blame and log used in sliced content tests.
    struct TestHandles {
        blame: TemplateHandle,
        log: TemplateHandle,
    }

    /// Create a temp git repo with a committed file, returning the `GitRepo`
    /// handle and the `TempDir` guard (must be kept alive).
    fn test_repo_with_file(filename: &str, content: &str) -> (Arc<crate::repo::GitRepo>, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let git_repo = git2::Repository::init(dir.path()).expect("git init");

        // Write the file.
        let file_path = dir.path().join(filename);
        std::fs::write(&file_path, content).expect("write file");

        // Stage and commit.
        let mut index = git_repo.index().expect("get index");
        index.add_path(std::path::Path::new(filename)).expect("add path");
        index.write().expect("write index");
        let tree_oid = index.write_tree().expect("write tree");
        let tree = git_repo.find_tree(tree_oid).expect("find tree");
        let sig = git2::Signature::now("Test Author", "test@example.com").expect("signature");
        git_repo
            .commit(Some("HEAD"), &sig, &sig, "initial commit", &tree, &[])
            .expect("commit");

        let repo = crate::repo::GitRepo::open(dir.path()).expect("open GitRepo");
        (Arc::new(repo), dir)
    }

    /// Build blame and log template handles for testing.
    fn git_handles() -> TestHandles {
        let mut b = HandleBuilder::new();
        let blame_key = b.register("git/blame", include_str!("templates/blame.md.j2"));
        let log_key = b.register("git/log", include_str!("templates/log.md.j2"));
        let engine = b.finish();
        TestHandles {
            blame: TemplateHandle::new(&engine, blame_key),
            log: TemplateHandle::new(&engine, log_key),
        }
    }

    /// Verifies that a range slice selects only the matching blame rows.
    #[test]
    fn sliced_blame_range_selects_subset() {
        let (repo, _dir) = test_repo_with_file("hello.txt", "line1\nline2\nline3\nline4\n");
        let ctx = repo::FileViewCtx {
            repo,
            rel_path: "hello.txt".into(),
        };
        let h = git_handles();
        let view = views::SlicedBlameView {
            ctx,
            spec: SliceSpec::Range(1, 2),
        };
        let output = String::from_utf8(h.blame.render_view(&view).expect("render")).expect("utf8");
        // Should contain blame table rows — at least the header and some data.
        assert!(output.contains("# Blame"), "expected blame header");
        // The full blame has 4 hunks (one per line or merged); slicing to 1-2
        // should produce fewer rows than the full file.
        let row_count = output.lines().filter(|l| l.starts_with('|') && l.contains('`')).count();
        assert!(row_count > 0, "expected at least one blame row");
        assert!(row_count <= 2, "expected at most 2 blame rows, got {row_count}");
    }

    /// Verifies that an out-of-range slice produces the empty-blame fallback.
    #[test]
    fn sliced_blame_empty_on_range_beyond_data() {
        let (repo, _dir) = test_repo_with_file("tiny.txt", "only line\n");
        let ctx = repo::FileViewCtx {
            repo,
            rel_path: "tiny.txt".into(),
        };
        let h = git_handles();
        let view = views::SlicedBlameView {
            ctx,
            spec: SliceSpec::Range(100, 200),
        };
        let output = String::from_utf8(h.blame.render_view(&view).expect("render")).expect("utf8");
        // Out-of-range slice produces the "no data" fallback.
        assert!(
            output.contains("No blame data available"),
            "expected empty-blame fallback, got: {output}"
        );
    }

    /// Verifies that a tail(1) slice yields exactly one blame row.
    #[test]
    fn sliced_blame_tail() {
        let (repo, _dir) = test_repo_with_file("four.txt", "a\nb\nc\nd\n");
        let ctx = repo::FileViewCtx {
            repo,
            rel_path: "four.txt".into(),
        };
        let h = git_handles();
        let view = views::SlicedBlameView {
            ctx,
            spec: SliceSpec::Tail(1),
        };
        let output = String::from_utf8(h.blame.render_view(&view).expect("render")).expect("utf8");
        let row_count = output.lines().filter(|l| l.starts_with('|') && l.contains('`')).count();
        assert_eq!(row_count, 1, "tail(1) should yield exactly 1 blame row");
    }

    /// Verifies that `Single(1)` returns exactly one log row.
    #[test]
    fn sliced_log_single_entry() {
        let (repo, _dir) = test_repo_with_file("hello.txt", "content\n");
        let ctx = repo::FileViewCtx {
            repo,
            rel_path: "hello.txt".into(),
        };
        let h = git_handles();
        // Repo has exactly 1 commit — Single(1) should return it.
        let view = views::SlicedLogView {
            ctx,
            spec: SliceSpec::Single(1),
        };
        let output = String::from_utf8(h.log.render_view(&view).expect("render")).expect("utf8");
        assert!(output.contains("# Log"), "expected log header");
        let row_count = output.lines().filter(|l| l.starts_with('|') && l.contains('`')).count();
        assert_eq!(row_count, 1, "expected exactly 1 log row");
    }

    /// Verifies that tail(N) returns all entries when fewer than N exist.
    #[test]
    fn sliced_log_tail_with_fewer_entries() {
        let (repo, _dir) = test_repo_with_file("hello.txt", "content\n");
        let ctx = repo::FileViewCtx {
            repo,
            rel_path: "hello.txt".into(),
        };
        let h = git_handles();
        // Ask for the last 100 entries but only 1 exists — should return that 1.
        let view = views::SlicedLogView {
            ctx,
            spec: SliceSpec::Tail(100),
        };
        let output = String::from_utf8(h.log.render_view(&view).expect("render")).expect("utf8");
        let row_count = output.lines().filter(|l| l.starts_with('|') && l.contains('`')).count();
        assert_eq!(row_count, 1, "tail(100) with 1 commit should yield 1 row");
    }

    /// Verifies that an out-of-range log slice produces the empty-log fallback.
    #[test]
    fn sliced_log_range_beyond_data() {
        let (repo, _dir) = test_repo_with_file("hello.txt", "content\n");
        let ctx = repo::FileViewCtx {
            repo,
            rel_path: "hello.txt".into(),
        };
        let h = git_handles();
        let view = views::SlicedLogView {
            ctx,
            spec: SliceSpec::Range(50, 100),
        };
        let output = String::from_utf8(h.log.render_view(&view).expect("render")).expect("utf8");
        assert!(
            output.contains("No history available"),
            "expected empty-log fallback, got: {output}"
        );
    }
}

/// Tests for `branch_segments_at_prefix` decomposition of slashed branch names.
mod branch_segment_tests {
    use std::sync::Arc;

    use rstest::rstest;

    use crate::repo::GitRepo;

    /// Create a temp repo and add the given branch names (on top of the initial commit).
    fn repo_with_branches(branch_names: &[&str]) -> (Arc<GitRepo>, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let git_repo = git2::Repository::init(dir.path()).expect("git init");

        let sig = git2::Signature::now("Test", "t@t.com").expect("sig");
        let tree_oid = git_repo.index().expect("idx").write_tree().expect("tree");
        let tree = git_repo.find_tree(tree_oid).expect("find tree");
        let commit_oid = git_repo
            .commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .expect("commit");
        let commit = git_repo.find_commit(commit_oid).expect("find commit");

        for name in branch_names {
            git_repo.branch(name, &commit, false).expect("create branch");
        }

        let repo = GitRepo::open(dir.path()).expect("open");
        (Arc::new(repo), dir)
    }

    /// Collect sorted (name, has_rename, has_unlink) tuples from `branch_segments_at_prefix`.
    fn segments(repo: &Arc<GitRepo>, prefix: &str) -> Vec<(String, bool, bool)> {
        let Some(nodes) = super::branches::branch_segments_at_prefix(repo, prefix).expect("should not error") else {
            return Vec::new();
        };
        let mut result: Vec<_> = nodes
            .iter()
            .map(|n| (n.name().to_owned(), n.renameable().is_some(), n.unlinkable().is_some()))
            .collect();
        result.sort();
        result
    }

    /// Verifies branch segment decomposition at various prefix depths.
    #[rstest]
    #[case::flat_at_root(&["alpha", "beta"], "", &[("alpha", true, true), ("beta", true, true)])]
    #[case::slashed_decomposes(&["feat/foo", "feat/bar", "fix/bug"], "", &[("feat", false, false), ("fix", false, false)])]
    #[case::nested_prefix(&["feat/foo", "feat/bar"], "feat/", &[("bar", true, true), ("foo", true, true)])]
    #[case::deep_root(&["a/b/c", "a/b/d"], "", &[("a", false, false)])]
    #[case::deep_mid(&["a/b/c", "a/b/d"], "a/", &[("b", false, false)])]
    #[case::deep_leaf(&["a/b/c", "a/b/d"], "a/b/", &[("c", true, true), ("d", true, true)])]
    #[case::nonexistent_prefix(&["feat/foo"], "nonexistent/", &[])]
    fn branch_segments(#[case] branches: &[&str], #[case] prefix: &str, #[case] expected: &[(&str, bool, bool)]) {
        let (repo, _dir) = repo_with_branches(branches);
        let segs = segments(&repo, prefix);
        let expected: Vec<(String, bool, bool)> = expected.iter().map(|(n, r, u)| ((*n).into(), *r, *u)).collect();
        // Use contains checks — HEAD branch (main/master) also appears at root.
        for entry in &expected {
            assert!(segs.contains(entry), "missing {entry:?} in {segs:?}");
        }
        // For non-root prefixes, exact match (no HEAD branch noise).
        if !prefix.is_empty() {
            assert_eq!(segs, expected);
        }
    }
}
/// Tests for `GitRepo::delete_branch` merge-safety checks.
mod delete_branch_tests {
    use std::sync::Arc;

    use crate::repo::GitRepo;

    /// Create a temp repo with an initial commit on the default branch,
    /// plus additional branches. Returns (repo, tempdir, initial_commit_oid).
    fn repo_with_branches(branch_names: &[&str]) -> (Arc<GitRepo>, tempfile::TempDir, git2::Oid) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let git_repo = git2::Repository::init(dir.path()).expect("git init");

        let sig = git2::Signature::now("Test", "t@t.com").expect("sig");
        let tree_oid = git_repo.index().expect("idx").write_tree().expect("tree");
        let tree = git_repo.find_tree(tree_oid).expect("find tree");
        let commit_oid = git_repo
            .commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .expect("commit");
        let commit = git_repo.find_commit(commit_oid).expect("find commit");

        for name in branch_names {
            git_repo.branch(name, &commit, false).expect("create branch");
        }

        let repo = GitRepo::open(dir.path()).expect("open");
        (Arc::new(repo), dir, commit_oid)
    }

    /// Verifies that a fully-merged branch can be deleted.
    #[test]
    fn delete_merged_branch() {
        let (repo, _dir, _) = repo_with_branches(&["merged-feature"]);
        // Branch points at same commit as HEAD → fully merged.
        repo.delete_branch("merged-feature")
            .expect("should delete merged branch");
        let branches = repo.branches().expect("list branches");
        assert!(!branches.contains(&"merged-feature".to_owned()));
    }

    /// Verifies that deleting the current HEAD branch is refused with `PermissionDenied`.
    #[test]
    fn delete_head_branch_refused() {
        let (repo, _dir, _) = repo_with_branches(&[]);
        let head = repo.head_branch();
        let err = repo.delete_branch(&head).expect_err("should refuse HEAD deletion");
        let io_err = err.downcast_ref::<std::io::Error>().expect("should be io::Error");
        assert_eq!(io_err.kind(), std::io::ErrorKind::PermissionDenied);
    }

    /// Verifies that deleting an unmerged branch is refused with `PermissionDenied`.
    #[test]
    fn delete_unmerged_branch_refused() {
        let (repo, dir, _) = repo_with_branches(&["diverged"]);
        // Add a commit only on `diverged` so it's not an ancestor of HEAD.
        {
            let git_repo = git2::Repository::open(dir.path()).expect("open raw");
            let sig = git2::Signature::now("Test", "t@t.com").expect("sig");
            let head_commit = git_repo.head().expect("head").peel_to_commit().expect("commit");
            let tree = head_commit.tree().expect("tree");
            let diverged_oid = git_repo
                .commit(None, &sig, &sig, "diverge", &tree, &[&head_commit])
                .expect("commit");
            let diverged_commit = git_repo.find_commit(diverged_oid).expect("find");
            git_repo
                .branch("diverged", &diverged_commit, true)
                .expect("update branch");
        }
        let err = repo.delete_branch("diverged").expect_err("should refuse unmerged");
        let io_err = err.downcast_ref::<std::io::Error>().expect("should be io::Error");
        assert_eq!(io_err.kind(), std::io::ErrorKind::PermissionDenied);
    }
}

/// Tests for `branch_tree_nodes` file tree browsing on branches.
mod branch_tree_tests {
    use std::sync::Arc;

    use rstest::rstest;

    use crate::repo::GitRepo;

    /// Create a temp repo with branches and committed files.
    fn repo_with_files(branch_names: &[&str], files: &[(&str, &str)]) -> (Arc<GitRepo>, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let git_repo = git2::Repository::init(dir.path()).expect("git init");

        let mut index = git_repo.index().expect("idx");
        for (path, content) in files {
            let full = dir.path().join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).expect("mkdir");
            }
            std::fs::write(&full, content).expect("write");
            index.add_path(std::path::Path::new(path)).expect("add");
        }
        index.write().expect("write index");
        let tree_oid = index.write_tree().expect("tree");
        let tree = git_repo.find_tree(tree_oid).expect("find tree");
        let sig = git2::Signature::now("Test", "t@t.com").expect("sig");
        let commit_oid = git_repo
            .commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .expect("commit");
        let commit = git_repo.find_commit(commit_oid).expect("find commit");

        for name in branch_names {
            git_repo.branch(name, &commit, false).expect("create branch");
        }

        let repo = GitRepo::open(dir.path()).expect("open");
        (Arc::new(repo), dir)
    }

    /// Collect sorted (name, is_file) pairs from `branch_tree_nodes`.
    fn tree_entries(repo: &Arc<GitRepo>, branch: &str, path: &str) -> Vec<(String, bool)> {
        let Some(nodes) = super::branches::branch_tree_nodes(repo, branch, path).expect("should not error") else {
            return Vec::new();
        };
        let mut result: Vec<_> = nodes
            .iter()
            .map(|n| (n.name().to_owned(), n.readable().is_some()))
            .collect();
        result.sort();
        result
    }

    /// Verifies tree listing at root and subtree levels on a branch.
    #[rstest]
    #[case::root_tree("dev", "", &[("README.md", true), ("src", false)])]
    #[case::subtree("dev", "src", &[("lib.rs", true), ("main.rs", true)])]
    fn tree_listing(#[case] branch: &str, #[case] path: &str, #[case] expected: &[(&str, bool)]) {
        let (repo, _dir) = repo_with_files(&["dev"], &[
            ("README.md", "hello"),
            ("src/main.rs", "fn main() {}"),
            ("src/lib.rs", "// lib"),
        ]);
        let entries = tree_entries(&repo, branch, path);
        let expected: Vec<(String, bool)> = expected.iter().map(|(n, r)| ((*n).into(), *r)).collect();
        assert_eq!(entries, expected);
    }

    /// Verifies that blob content can be read from a branch's tree.
    #[test]
    fn blob_content_readable() {
        let (repo, _dir) = repo_with_files(&["dev"], &[("hello.txt", "world")]);
        let content = repo.blob_at_ref("dev", "hello.txt").expect("read blob");
        assert_eq!(content, b"world");
    }
}
