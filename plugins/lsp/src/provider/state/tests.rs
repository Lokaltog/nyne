use rstest::rstest;

use super::*;

/// Verifies that sub-route detection uses the configured directory names,
/// not hardcoded defaults. Custom VFS config values (e.g. `_test` suffix)
/// must be recognized as sub-routes alongside feature-derived dirs.
#[rstest]
// Default config — canonical dir names.
#[case::default_actions("actions", "rename", &["Foo@", "actions"], &["Foo@"], Some("actions"))]
#[case::default_rename("actions", "rename", &["Foo@", "rename"], &["Foo@"], Some("rename"))]
// Custom config — suffixed dir names.
#[case::custom_actions("actions_test", "rename_test", &["Foo@", "actions_test"], &["Foo@"], Some("actions_test"))]
#[case::custom_rename("actions_test", "rename_test", &["Foo@", "rename_test"], &["Foo@"], Some("rename_test"))]
// Feature-derived dirs (callers, deps, etc.) — always recognized regardless of config.
#[case::feature_callers("actions", "rename", &["Foo@", "callers"], &["Foo@"], Some("callers"))]
#[case::feature_deps("actions", "rename", &["Foo@", "deps"], &["Foo@"], Some("deps"))]
#[case::feature_references("actions", "rename", &["Foo@", "references"], &["Foo@"], Some("references"))]
// Non-sub-route segments — must NOT match.
#[case::body_file("actions", "rename", &["Foo@", "body.rs"], &["Foo@", "body.rs"], None)]
#[case::fragment_only("actions", "rename", &["Foo@"], &["Foo@"], None)]
#[case::nested_fragment("actions", "rename", &["Foo@", "Bar@"], &["Foo@", "Bar@"], None)]
// Custom config must NOT match the default names.
#[case::default_name_with_custom_config("actions_test", "rename_test", &["Foo@", "actions"], &["Foo@", "actions"], None)]
#[case::default_rename_with_custom_config("actions_test", "rename_test", &["Foo@", "rename"], &["Foo@", "rename"], None)]
fn sub_route_detection_uses_configured_dirs(
    #[case] actions_dir: &str,
    #[case] rename_dir: &str,
    #[case] segments: &[&str],
    #[case] expected_frag: &[&str],
    #[case] expected_sub: Option<&str>,
) {
    let segments: Vec<String> = segments.iter().map(|s| (*s).to_owned()).collect();
    let (frag, sub) = split_sub_route(&segments, actions_dir, rename_dir);
    assert_eq!(frag.iter().map(String::as_str).collect::<Vec<_>>(), expected_frag);
    assert_eq!(sub, expected_sub);
}
