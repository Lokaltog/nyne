use std::path::PathBuf;
use std::sync::Arc;

use nyne::path_filter::PathFilter;
use nyne::router::{AffectedFiles, MemFs, NamedNode, Next, Op, RenameContext, Renameable, Request, RouteTree};
use nyne_visibility::Visibility;
use rstest::rstest;
use tempfile::TempDir;

use super::*;

struct StubRenameable;

impl Renameable for StubRenameable {
    fn rename(&self, ctx: &RenameContext<'_>) -> color_eyre::eyre::Result<AffectedFiles> {
        Ok(vec![ctx.source.to_path_buf(), ctx.target.to_path_buf()])
    }
}

#[rstest]
#[case::strips_both("src/Foo@", "src/Bar@", "src/Foo", "src/Bar")]
#[case::strips_target_only("src/Foo", "src/Bar@", "src/Foo", "src/Bar")]
#[case::no_suffix_passthrough("src/Foo", "src/Bar", "src/Foo", "src/Bar")]
#[case::empty_after_strip_keeps_original("@", "src/Bar@", "@", "src/Bar")]
fn companion_renameable_strips_suffix(
    #[case] source: &str,
    #[case] target: &str,
    #[case] expected_source: &str,
    #[case] expected_target: &str,
) {
    let wrapper = CompanionRenameable {
        inner: Arc::new(StubRenameable),
        suffix: Arc::from("@"),
    };
    let src = PathBuf::from(source);
    let tgt = PathBuf::from(target);
    let ctx = RenameContext {
        source: &src,
        target: &tgt,
    };
    let affected = wrapper.rename(&ctx).unwrap();
    assert_eq!(affected, vec![
        PathBuf::from(expected_source),
        PathBuf::from(expected_target),
    ]);
}

/// Build a `CompanionProvider` whose `PathFilter` is rooted at `tmp`
/// and ignores any patterns in `gitignore`. Route trees are empty —
/// the gate tests only assert state before tree dispatch.
fn provider_with_filter(tmp: &TempDir, gitignore: &str) -> CompanionProvider {
    std::fs::write(tmp.path().join(".gitignore"), gitignore).expect("write .gitignore");
    let path_filter = Arc::new(PathFilter::build(tmp.path(), &[]));
    CompanionProvider {
        suffix: Arc::from("@"),
        file_tree: RouteTree::builder().build(),
        dir_tree: RouteTree::builder().build(),
        mount_tree: RouteTree::builder().build(),
        fs: Arc::new(MemFs::new()),
        path_filter: Some(path_filter),
    }
}

/// Build a readdir `Request` at `dir` pre-populated with file entries
/// and `Force` visibility — mirrors what upstream providers would
/// leave for `emit_companion_dirs` to decorate.
fn readdir_request(dir: PathBuf, files: &[&str]) -> Request {
    let mut req = Request::new(dir, Op::Readdir);
    req.set_state(Visibility::Force);
    for name in files {
        req.nodes.add(NamedNode::file((*name).to_owned()));
    }
    req
}

/// Expected outcome for path-filter gate tests. Replaces a raw `bool`
/// at the call site for clearer case intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expected {
    /// The path is gitignored — companion decoration is bypassed.
    Bypassed,
    /// The path is allowed — companion decoration is applied.
    Decorated,
}

#[rstest]
#[case::ignored_dir("node_modules/foo", Expected::Bypassed)]
#[case::unignored_dir("src", Expected::Decorated)]
fn emit_companion_dirs_respects_path_filter(#[case] subdir: &str, #[case] expected: Expected) {
    let tmp = TempDir::new().unwrap();
    let provider = provider_with_filter(&tmp, "node_modules/\n");
    let mut req = readdir_request(tmp.path().join(subdir), &["bar.js"]);
    provider.emit_companion_dirs(&mut req);
    assert_eq!(
        req.nodes.iter().any(|n| n.name() == "bar.js@"),
        expected == Expected::Decorated,
    );
}

#[rstest]
#[case::ignored_source("node_modules/foo/bar.js@/symbols/Foo@/body.rs", Expected::Bypassed)]
#[case::unignored_source("src/main.rs@/symbols/Foo@/body.rs", Expected::Decorated)]
fn rewrite_companion_path_respects_path_filter(#[case] rel: &str, #[case] expected: Expected) {
    let tmp = TempDir::new().unwrap();
    let provider = provider_with_filter(&tmp, "node_modules/\n");
    let mut req = Request::new(tmp.path().join(rel), Op::Readdir);
    provider.rewrite_companion_path(&mut req);
    assert_eq!(req.companion().is_some(), expected == Expected::Decorated);
}

#[rstest]
#[case::ignored_parent(("node_modules/foo", "bar.js@"), Expected::Bypassed)]
#[case::unignored_parent(("src", "main.rs@"), Expected::Decorated)]
fn accept_lookup_outer_suffix_respects_path_filter(#[case] lookup: (&str, &str), #[case] expected: Expected) {
    let (dir, name) = lookup;
    let tmp = TempDir::new().unwrap();
    let provider = provider_with_filter(&tmp, "node_modules/\n");
    let mut req = Request::new(tmp.path().join(dir), Op::Lookup { name: name.to_owned() });
    provider.accept_lookup(&mut req, &Next::empty(), name).unwrap();
    let decorated = expected == Expected::Decorated;
    assert_eq!(req.companion().is_some(), decorated);
    assert_eq!(req.nodes.iter().any(|n| n.name() == name), decorated);
}
