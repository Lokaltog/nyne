use std::path::{Path, PathBuf};

use rstest::rstest;

use super::*;

const SOURCE_ROOT: &str = "/tmp/nyne/proc/123/fs/merged/home/user/project";

/// Build a test resolver mapping `/code` to a source root path.
fn test_resolver() -> PathResolver { PathResolver::new("/code".into(), SOURCE_ROOT.into()) }

/// `rewrite` translates display-root paths to source-root paths and
/// passes non-matching paths through unchanged.
#[rstest]
#[case::prefix_replaced("/code/src/main.rs", "/tmp/nyne/proc/123/fs/merged/home/user/project/src/main.rs")]
#[case::root_itself("/code", "/tmp/nyne/proc/123/fs/merged/home/user/project")]
#[case::non_matching_unchanged("/other/path/file.rs", "/other/path/file.rs")]
fn rewrite_translates_display_paths(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(test_resolver().rewrite(input), PathBuf::from(expected));
}

/// `rewrite_to_fuse` translates source-root paths back to display-root
/// paths and passes non-matching paths through unchanged.
#[rstest]
#[case::prefix_replaced("/tmp/nyne/proc/123/fs/merged/home/user/project/src/main.rs", "/code/src/main.rs")]
#[case::root_itself("/tmp/nyne/proc/123/fs/merged/home/user/project", "/code")]
#[case::non_matching_unchanged("/other/path/file.rs", "/other/path/file.rs")]
fn rewrite_to_fuse_translates_source_paths(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(
        test_resolver().rewrite_to_fuse(Path::new(input)),
        PathBuf::from(expected)
    );
}
