use std::path::{Path, PathBuf};

use super::*;

/// Build a test resolver mapping `/code` to a source root path.
fn test_resolver() -> PathResolver {
    PathResolver::new("/code".into(), "/tmp/nyne/proc/123/fs/merged/home/user/project".into())
}

/// Tests that rewrite replaces the display root prefix with the source root.
#[test]
fn rewrite_replaces_display_root_prefix() {
    assert_eq!(
        test_resolver().rewrite("/code/src/main.rs"),
        PathBuf::from("/tmp/nyne/proc/123/fs/merged/home/user/project/src/main.rs")
    );
}

/// Tests that rewrite passes through paths outside the display root.
#[test]
fn rewrite_leaves_non_root_paths_unchanged() {
    assert_eq!(
        test_resolver().rewrite("/other/path/file.rs"),
        PathBuf::from("/other/path/file.rs")
    );
}

/// Tests that rewrite handles the exact display root path.
#[test]
fn rewrite_handles_root_itself() {
    assert_eq!(
        test_resolver().rewrite("/code"),
        PathBuf::from("/tmp/nyne/proc/123/fs/merged/home/user/project")
    );
}

/// Tests that `rewrite_to_fuse` replaces the source root prefix with the display root.
#[test]
fn rewrite_to_display_replaces_source_prefix() {
    assert_eq!(
        test_resolver().rewrite_to_fuse(Path::new("/tmp/nyne/proc/123/fs/merged/home/user/project/src/main.rs")),
        PathBuf::from("/code/src/main.rs")
    );
}

/// Tests that `rewrite_to_fuse` passes through paths outside the source root.
#[test]
fn rewrite_to_display_leaves_non_source_paths_unchanged() {
    assert_eq!(
        test_resolver().rewrite_to_fuse(Path::new("/other/path/file.rs")),
        PathBuf::from("/other/path/file.rs")
    );
}

/// Tests that `rewrite_to_fuse` handles the exact source root path.
#[test]
fn rewrite_to_display_handles_source_root_itself() {
    assert_eq!(
        test_resolver().rewrite_to_fuse(Path::new("/tmp/nyne/proc/123/fs/merged/home/user/project")),
        PathBuf::from("/code")
    );
}
