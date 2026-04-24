use super::*;
use crate::history;

/// Shared helpers for git-backed test fixtures.
mod test_repo {
    use std::path::Path;
    use std::sync::Arc;

    use tempfile::TempDir;

    use crate::repo::Repo;

    /// A built git repo plus the `TempDir` guard (must be kept alive).
    pub(super) struct TestRepo {
        pub repo: Arc<Repo>,
        pub dir: TempDir,
    }

    /// Builder for [`TestRepo`] — collects files and branches, then
    /// commits them all in a single initial commit on HEAD.
    #[derive(Default)]
    pub(super) struct Builder<'a> {
        files: Vec<(&'a str, &'a str)>,
        branches: Vec<&'a str>,
    }

    impl<'a> Builder<'a> {
        pub fn file(mut self, path: &'a str, content: &'a str) -> Self {
            self.files.push((path, content));
            self
        }

        pub fn files(mut self, files: &[(&'a str, &'a str)]) -> Self {
            self.files.extend_from_slice(files);
            self
        }

        pub fn branches(mut self, names: &[&'a str]) -> Self {
            self.branches.extend_from_slice(names);
            self
        }

        pub fn build(self) -> TestRepo {
            let dir = tempfile::tempdir().expect("create temp dir");
            let git_repo = git2::Repository::init(dir.path()).expect("git init");

            let mut index = git_repo.index().expect("get index");
            for (path, content) in &self.files {
                let full = dir.path().join(path);
                if let Some(parent) = full.parent() {
                    std::fs::create_dir_all(parent).expect("mkdir");
                }
                std::fs::write(&full, content).expect("write file");
                index.add_path(Path::new(path)).expect("add path");
            }
            index.write().expect("write index");
            let tree_oid = index.write_tree().expect("write tree");
            let tree = git_repo.find_tree(tree_oid).expect("find tree");
            let sig = git2::Signature::now("Test Author", "test@example.com").expect("signature");
            let commit_oid = git_repo
                .commit(Some("HEAD"), &sig, &sig, "initial commit", &tree, &[])
                .expect("commit");
            let commit = git_repo.find_commit(commit_oid).expect("find commit");

            for name in &self.branches {
                git_repo.branch(name, &commit, false).expect("create branch");
            }

            let repo = Repo::open(dir.path()).expect("open repo");
            TestRepo {
                repo: Arc::new(repo),
                dir,
            }
        }
    }

    pub(super) fn builder<'a>() -> Builder<'a> { Builder::default() }
}
/// Tests that `hunk_overlaps_range` correctly detects overlap between blame hunks and line ranges.
mod hunk_overlap_tests {
    use nyne::SymbolLineRange;
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

    /// Check whether a blame hunk overlaps a 1-based inclusive line range.
    const fn hunk_overlaps_range(hunk: &history::BlameHunk, range: &SymbolLineRange) -> bool {
        hunk.start_line <= range.end && hunk.end_line >= range.start
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
            hunk_overlaps_range(&hunk(lines), &range),
            expected,
            "hunk {lines} vs range 10-20"
        );
    }
}

/// Tests for `SymbolLineRange` construction and formatting.
mod symbol_line_range_tests {
    use nyne::SymbolLineRange;

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
            commit: crate::commit::CommitInfo {
                hash: "abc1234".into(),
                author: "dev".into(),
                date: "2024-01-15".into(),
                message: message.into(),
                epoch_secs: 0,
            },
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

    use nyne::SliceSpec;
    use nyne::templates::{HandleBuilder, LazyView, TemplateHandle};
    use rstest::rstest;

    use crate::history::HistoryQueries as _;

    /// Template handles for blame and log used in sliced content tests.
    struct TestHandles {
        blame: TemplateHandle,
        log: TemplateHandle,
    }

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

    fn sliced_blame_view(
        repo: Arc<crate::repo::Repo>,
        rel: &str,
        spec: SliceSpec,
    ) -> LazyView<impl Fn(&nyne::templates::TemplateEngine, &str) -> color_eyre::eyre::Result<Vec<u8>> + Send + Sync>
    {
        let rel = rel.to_owned();
        LazyView::new(move |engine: &nyne::templates::TemplateEngine, tmpl: &str| {
            let hunks = repo.blame(&rel)?;
            Ok(engine.render_bytes(
                tmpl,
                &minijinja::context!(data => crate::history::slice_blame_hunks(hunks, &spec)),
            ))
        })
    }

    fn sliced_log_view(
        repo: Arc<crate::repo::Repo>,
        rel: &str,
        spec: SliceSpec,
    ) -> LazyView<impl Fn(&nyne::templates::TemplateEngine, &str) -> color_eyre::eyre::Result<Vec<u8>> + Send + Sync>
    {
        let rel = rel.to_owned();
        LazyView::new(move |engine: &nyne::templates::TemplateEngine, tmpl: &str| {
            let entries = repo.file_history(&rel, 200)?;
            let sliced = spec.apply(&entries);
            Ok(engine.render_bytes(tmpl, &minijinja::context!(data => sliced)))
        })
    }

    /// Expected output shape for sliced blame/log rendering.
    enum Expect {
        /// Row count must lie within the inclusive range.
        RowsBetween(usize, usize),
        /// Output must contain the given substring.
        Contains(&'static str),
    }

    impl Expect {
        fn rows(n: usize) -> Self { Self::RowsBetween(n, n) }

        fn check(&self, output: &str) {
            let rows = output.lines().filter(|l| l.starts_with('|') && l.contains('`')).count();
            match *self {
                Self::RowsBetween(min, max) => {
                    assert!(
                        rows >= min && rows <= max,
                        "expected {min}..={max} rows, got {rows} in:\n{output}"
                    );
                }
                Self::Contains(needle) => {
                    assert!(output.contains(needle), "expected substring `{needle}` in:\n{output}");
                }
            }
        }
    }

    #[rstest]
    #[case::range_selects_subset(
        "hello.txt",
        "line1\nline2\nline3\nline4\n",
        SliceSpec::Range(1, 2),
        Expect::RowsBetween(1, 2)
    )]
    #[case::empty_on_range_beyond_data(
        "tiny.txt",
        "only line\n",
        SliceSpec::Range(100, 200),
        Expect::Contains("No blame data available")
    )]
    #[case::tail("four.txt", "a\nb\nc\nd\n", SliceSpec::Tail(1), Expect::rows(1))]
    fn sliced_blame(#[case] filename: &str, #[case] content: &str, #[case] spec: SliceSpec, #[case] expect: Expect) {
        let repo = super::test_repo::builder().file(filename, content).build();
        let h = git_handles();
        let view = sliced_blame_view(repo.repo.clone(), filename, spec);
        let output = String::from_utf8(h.blame.render_view(&view).expect("render")).expect("utf8");
        expect.check(&output);
    }

    #[rstest]
    #[case::single_entry("hello.txt", "content\n", SliceSpec::Single(1), Expect::rows(1))]
    #[case::tail_with_fewer_entries("hello.txt", "content\n", SliceSpec::Tail(100), Expect::rows(1))]
    #[case::range_beyond_data(
        "hello.txt",
        "content\n",
        SliceSpec::Range(50, 100),
        Expect::Contains("No history available")
    )]
    fn sliced_log(#[case] filename: &str, #[case] content: &str, #[case] spec: SliceSpec, #[case] expect: Expect) {
        let repo = super::test_repo::builder().file(filename, content).build();
        let h = git_handles();
        let view = sliced_log_view(repo.repo.clone(), filename, spec);
        let output = String::from_utf8(h.log.render_view(&view).expect("render")).expect("utf8");
        expect.check(&output);
    }
}

/// Tests for `branch_segments_at_prefix` decomposition of slashed branch names.
mod branch_segment_tests {
    use std::sync::Arc;

    use rstest::rstest;

    use crate::repo::Repo;

    /// Collect sorted (name, `has_rename`, `has_unlink`) tuples from `branch_segments_at_prefix`.
    fn segments(repo: &Arc<Repo>, prefix: &str) -> Vec<(String, bool, bool)> {
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
        let built = super::test_repo::builder().branches(branches).build();
        let segs = segments(&built.repo, prefix);
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
    /// Verifies that a fully-merged branch can be deleted.
    #[test]
    fn delete_merged_branch() {
        let built = super::test_repo::builder().branches(&["merged-feature"]).build();
        // Branch points at same commit as HEAD → fully merged.
        built
            .repo
            .delete_branch("merged-feature")
            .expect("should delete merged branch");
        let branches = built.repo.branches().expect("list branches");
        assert!(!branches.contains(&"merged-feature".to_owned()));
    }

    /// Verifies that deleting the current HEAD branch is refused with `PermissionDenied`.
    #[test]
    fn delete_head_branch_refused() {
        let built = super::test_repo::builder().build();
        let head = built.repo.head_branch();
        let err = built
            .repo
            .delete_branch(&head)
            .expect_err("should refuse HEAD deletion");
        let io_err = err.downcast_ref::<std::io::Error>().expect("should be io::Error");
        assert_eq!(io_err.kind(), std::io::ErrorKind::PermissionDenied);
    }

    /// Verifies that deleting an unmerged branch is refused with `PermissionDenied`.
    #[test]
    fn delete_unmerged_branch_refused() {
        let built = super::test_repo::builder().branches(&["diverged"]).build();
        // Add a commit only on `diverged` so it's not an ancestor of HEAD.
        {
            let git_repo = git2::Repository::open(built.dir.path()).expect("open raw");
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
        let err = built
            .repo
            .delete_branch("diverged")
            .expect_err("should refuse unmerged");
        let io_err = err.downcast_ref::<std::io::Error>().expect("should be io::Error");
        assert_eq!(io_err.kind(), std::io::ErrorKind::PermissionDenied);
    }
}

/// Tests for `branch_tree_nodes` file tree browsing on branches.
mod branch_tree_tests {
    use std::sync::Arc;

    use rstest::rstest;

    use crate::repo::Repo;

    /// Collect sorted (name, `is_file`) pairs from `branch_tree_nodes`.
    fn tree_entries(repo: &Arc<Repo>, branch: &str, path: &str) -> Vec<(String, bool)> {
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
        let built = super::test_repo::builder()
            .branches(&["dev"])
            .files(&[
                ("README.md", "hello"),
                ("src/main.rs", "fn main() {}"),
                ("src/lib.rs", "// lib"),
            ])
            .build();
        let entries = tree_entries(&built.repo, branch, path);
        let expected: Vec<(String, bool)> = expected.iter().map(|(n, r)| ((*n).into(), *r)).collect();
        assert_eq!(entries, expected);
    }

    /// Verifies that blob content can be read from a branch's tree.
    #[test]
    fn blob_content_readable() {
        let built = super::test_repo::builder()
            .branches(&["dev"])
            .file("hello.txt", "world")
            .build();
        assert_eq!(built.repo.blob_at_ref("dev", "hello.txt").expect("read blob"), b"world");
    }

    mod slice_blame_hunks_tests {
        use nyne::SliceSpec;
        use rstest::rstest;

        use crate::commit::CommitInfo;
        use crate::history::{BlameHunk, slice_blame_hunks};

        fn hunk(start: usize, end: usize) -> BlameHunk {
            BlameHunk {
                start_line: start,
                end_line: end,
                commit: CommitInfo {
                    hash: format!("h{start}"),
                    author: "dev".into(),
                    date: "2024-01-15".into(),
                    message: "test".into(),
                    epoch_secs: 0,
                },
            }
        }

        fn lines(hunks: &[BlameHunk]) -> Vec<(usize, usize)> {
            hunks.iter().map(|h| (h.start_line, h.end_line)).collect()
        }

        /// Four hunks: 1-3, 4-6, 7-9, 10-12
        fn sample_hunks() -> Vec<BlameHunk> { vec![hunk(1, 3), hunk(4, 6), hunk(7, 9), hunk(10, 12)] }

        #[rstest]
        #[case::exact_hunk(SliceSpec::Range(4, 6), &[(4, 6)])]
        #[case::clips_start(SliceSpec::Range(2, 4), &[(2, 3), (4, 4)])]
        #[case::clips_end(SliceSpec::Range(5, 8), &[(5, 6), (7, 8)])]
        #[case::clips_both(SliceSpec::Range(2, 11), &[(2, 3), (4, 6), (7, 9), (10, 11)])]
        #[case::single_line(SliceSpec::Single(5), &[(5, 5)])]
        #[case::beyond_end(SliceSpec::Range(13, 20), &[])]
        #[case::full_range(SliceSpec::Range(1, 12), &[(1, 3), (4, 6), (7, 9), (10, 12)])]
        #[case::tail_one(SliceSpec::Tail(1), &[(12, 12)])]
        #[case::tail_all(SliceSpec::Tail(100), &[(1, 3), (4, 6), (7, 9), (10, 12)])]
        fn slice_by_line_range(#[case] spec: SliceSpec, #[case] expected: &[(usize, usize)]) {
            let result = slice_blame_hunks(sample_hunks(), &spec);
            assert_eq!(lines(&result), expected);
        }

        #[test]
        fn empty_input() {
            assert!(slice_blame_hunks(vec![], &SliceSpec::Range(1, 10)).is_empty());
        }
    }
}
