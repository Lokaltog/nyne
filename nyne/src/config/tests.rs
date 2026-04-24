use rstest::rstest;

use super::*;

/// Load a fixture TOML file and deserialize into `NyneConfig`.
fn load_fixture(name: &str) -> NyneConfig {
    toml::from_str(&crate::load_fixture!("config", name))
        .unwrap_or_else(|e| panic!("failed to parse fixture {name}: {e}"))
}

/// Verifies that the default `NyneConfig` passes validation.
#[test]
fn default_config_is_valid() {
    let config = NyneConfig::default();
    config.validate().expect("default config should be valid");
    insta::assert_toml_snapshot!(config);
}

/// Tests that fixture TOML files deserialize correctly into `NyneConfig`.
#[rstest]
#[case::minimal("minimal.toml", "deserialize_minimal_config")]
#[case::mount("mount.toml", "deserialize_mount_config")]
fn deserialize_fixture(#[case] fixture: &str, #[case] snapshot: &str) {
    let config = load_fixture(fixture);
    insta::assert_toml_snapshot!(snapshot, config);
}

/// Tests that mount config with excluded patterns preserves the exclusion list.
#[test]
fn deserialize_mount_config_with_exclusions() {
    let config = load_fixture("mount_with_exclusions.toml");
    let mount = config.mount.expect("mount should be present");
    assert_eq!(mount.excluded_patterns, vec!["target", "*.o", ".git"]);
}

/// Verifies that TOML with unknown fields or invalid values is rejected by deserialization.
#[rstest]
#[case::unknown_top_level("unknown_key = \"value\"")]
#[case::unknown_mount("[mount]\nsource_dir = \"/tmp/src\"\nmountpoint = \"/tmp/mnt\"\nbogus = true")]
#[case::unknown_repository("[repository]\nbogus = true")]
#[case::unknown_sandbox("[sandbox]\nbogus = true")]
#[case::unknown_bind_mount("[[sandbox.bind_mounts]]\nsource = \"/src\"\ntarget = \"/dst\"\nbogus = true")]
#[case::invalid_bind_mount_flag("[[sandbox.bind_mounts]]\nsource = \"/src\"\ntarget = \"/dst\"\nflags = [\"bogus\"]")]
#[case::unknown_agent_files("[agent_files]\nbogus = true")]
fn reject_invalid_config(#[case] toml_input: &str) {
    let result: std::result::Result<NyneConfig, _> = toml::from_str(toml_input);
    assert!(result.is_err(), "invalid config should be rejected: {toml_input}");
}

/// Verifies that `agent_files.filenames` deserializes correctly across all supported
/// fixture shapes. `None` in `expected` means the result should equal
/// [`default_agent_filenames()`]; `Some(list)` pins an exact custom list.
#[rstest]
#[case::defaults_when_omitted("minimal.toml", None)]
#[case::section_without_filenames("agent_files_section_only.toml", None)]
#[case::custom("agent_files_custom.toml", Some(&["COPILOT.md"] as &[&str]))]
#[case::empty("agent_files_empty.toml", Some(&[] as &[&str]))]
fn agent_files_deserialization(#[case] fixture: &str, #[case] expected: Option<&[&str]>) {
    let config = load_fixture(fixture);
    let actual = &config.agent_files.filenames;
    match expected {
        None => assert_eq!(actual, &default_agent_filenames()),
        Some(list) => assert_eq!(actual.iter().map(String::as_str).collect::<Vec<_>>(), list),
    }
}

/// Verifies that the `AgentFilesConfig` default has the expected filenames.
#[test]
fn agent_files_default_is_valid() {
    let config = AgentFilesConfig::default();
    assert_eq!(config.filenames, default_agent_filenames());
}

/// [`NyneConfig::repository.storage_strategy`] deserializes from explicit values, defaulting when omitted.
#[rstest]
#[case::defaults_when_omitted("", StorageStrategy::Passthrough)]
#[case::passthrough("[repository]\nstorage_strategy = \"passthrough\"", StorageStrategy::Passthrough)]
#[case::snapshot("[repository]\nstorage_strategy = \"snapshot\"", StorageStrategy::Snapshot)]
#[case::hardlink("[repository]\nstorage_strategy = \"hardlink\"", StorageStrategy::Hardlink)]
fn repository_storage_strategy(#[case] toml_input: &str, #[case] expected: StorageStrategy) {
    let config: NyneConfig = toml::from_str(toml_input).unwrap();
    assert_eq!(config.repository.storage_strategy, expected);
}

/// Verifies that `sandbox.hostname` defaults to `nyne-sandbox` when unset and
/// picks up custom values when specified.
#[rstest]
#[case::defaults_when_omitted("minimal.toml", "nyne-sandbox")]
#[case::custom("sandbox_custom_hostname.toml", "my-sandbox")]
fn sandbox_hostname(#[case] fixture: &str, #[case] expected: &str) {
    let config = load_fixture(fixture);
    assert_eq!(config.sandbox.hostname, expected);
}

/// Tests that bind mounts with source, target, and flags deserialize correctly.
#[test]
fn sandbox_bind_mounts() {
    let config = load_fixture("sandbox_bind_mounts.toml");
    assert_eq!(config.sandbox.bind_mounts.len(), 2);

    let first = &config.sandbox.bind_mounts[0];
    assert_eq!(first.source, PathBuf::from("/data/models"));
    assert_eq!(first.target, PathBuf::from("/models"));
    assert_eq!(first.flags, vec![BindMountFlag::ReadOnly, BindMountFlag::Noexec]);

    let second = &config.sandbox.bind_mounts[1];
    assert_eq!(second.source, PathBuf::from("/tmp/scratch"));
    assert_eq!(second.target, PathBuf::from("/scratch"));
    assert!(second.flags.is_empty());
}

/// Tests that all four bind mount flag variants deserialize correctly.
#[test]
fn sandbox_bind_mounts_all_flags() {
    let config = load_fixture("sandbox_bind_mounts_all_flags.toml");
    assert_eq!(config.sandbox.bind_mounts.len(), 1);
    let bm = &config.sandbox.bind_mounts[0];
    assert_eq!(bm.flags, vec![
        BindMountFlag::ReadOnly,
        BindMountFlag::Noexec,
        BindMountFlag::Nosuid,
        BindMountFlag::Nodev,
    ]);
}

/// Verifies that bind mount flags convert to the correct kernel `MountFlags` bitset.
#[test]
fn sandbox_bind_mounts_mount_flags() {
    use rustix::mount::MountFlags;

    let config = load_fixture("sandbox_bind_mounts.toml");

    let first_flags = config.sandbox.bind_mounts[0].mount_flags();
    assert_eq!(first_flags, Some(MountFlags::RDONLY | MountFlags::NOEXEC));

    let second_flags = config.sandbox.bind_mounts[1].mount_flags();
    assert_eq!(second_flags, None);
}

/// Tests that sandbox environment variables deserialize from TOML.
#[test]
fn sandbox_env() {
    let config = load_fixture("sandbox_env.toml");
    assert_eq!(config.sandbox.env.len(), 2);
    assert_eq!(config.sandbox.env["MY_CUSTOM_VAR"], "hello");
    assert_eq!(config.sandbox.env["ANOTHER_VAR"], "world");
}

/// Tests that sandbox env defaults to an empty map when omitted.
#[test]
fn sandbox_env_defaults_empty() {
    let config = load_fixture("minimal.toml");
    assert!(config.sandbox.env.is_empty());
}

/// Verifies that a `NyneConfig` with custom agent filenames passes validation.
#[test]
fn agent_files_config_validates() {
    let config = NyneConfig {
        agent_files: AgentFilesConfig {
            filenames: vec!["CUSTOM.md".to_owned()],
        },
        ..NyneConfig::default()
    };
    config.validate().expect("agent_files config should be valid");
}
#[test]
fn default_matches_empty_toml_deserialization() {
    let from_toml: NyneConfig = toml::from_str("").expect("empty TOML should deserialize");
    let from_default = NyneConfig::default();

    let toml_value = toml::Value::try_from(&from_toml).expect("serialization of deserialized config");
    let default_value = toml::Value::try_from(&from_default).expect("serialization of default config");

    assert_eq!(
        toml_value, default_value,
        "NyneConfig::default() diverges from serde defaults -- \
         update the manual Default impl to match #[serde(default)] attributes"
    );
}
