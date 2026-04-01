use std::path::PathBuf;
use std::sync::Arc;

use nyne::router::{AffectedFiles, RenameContext, Renameable};
use rstest::rstest;

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
