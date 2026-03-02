use std::path::{Path, PathBuf};

use super::*;

fn test_resolver() -> LspPathResolver {
    LspPathResolver::new("/code".into(), "/tmp/nyne-merged-123/home/user/project".into())
}

#[test]
fn rewrite_replaces_display_root_prefix() {
    assert_eq!(
        test_resolver().rewrite("/code/src/main.rs"),
        PathBuf::from("/tmp/nyne-merged-123/home/user/project/src/main.rs")
    );
}

#[test]
fn rewrite_leaves_non_root_paths_unchanged() {
    assert_eq!(
        test_resolver().rewrite("/other/path/file.rs"),
        PathBuf::from("/other/path/file.rs")
    );
}

#[test]
fn rewrite_handles_root_itself() {
    assert_eq!(
        test_resolver().rewrite("/code"),
        PathBuf::from("/tmp/nyne-merged-123/home/user/project")
    );
}

#[test]
fn rewrite_to_display_replaces_overlay_prefix() {
    assert_eq!(
        test_resolver().rewrite_to_fuse(Path::new("/tmp/nyne-merged-123/home/user/project/src/main.rs")),
        PathBuf::from("/code/src/main.rs")
    );
}

#[test]
fn rewrite_to_display_leaves_non_overlay_paths_unchanged() {
    assert_eq!(
        test_resolver().rewrite_to_fuse(Path::new("/other/path/file.rs")),
        PathBuf::from("/other/path/file.rs")
    );
}

#[test]
fn rewrite_to_display_handles_overlay_root_itself() {
    assert_eq!(
        test_resolver().rewrite_to_fuse(Path::new("/tmp/nyne-merged-123/home/user/project")),
        PathBuf::from("/code")
    );
}
