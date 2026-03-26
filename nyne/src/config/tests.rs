use rstest::rstest;

use super::*;

/// Load a fixture TOML file and deserialize into `NyneConfig`.
fn load_fixture(name: &str) -> NyneConfig {
    let content = crate::test_support::load_fixture("config", name);
    toml::from_str(&content).unwrap_or_else(|e| panic!("failed to parse fixture {name}: {e}"))
}

/// Verifies that the default NyneConfig passes validation.
#[test]
fn default_config_is_valid() {
    let config = NyneConfig::default();
    config.validate().expect("default config should be valid");
    insta::assert_toml_snapshot!(config);
}

/// Tests that a minimal TOML fixture deserializes correctly.
#[test]
fn deserialize_minimal_config() {
    let config = load_fixture("minimal.toml");
    insta::assert_toml_snapshot!(config);
}

/// Tests that a mount configuration TOML fixture deserializes correctly.
#[test]
fn deserialize_mount_config() {
    let config = load_fixture("mount.toml");
    insta::assert_toml_snapshot!(config);
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

/// Tests that agent_files defaults to CLAUDE.md and AGENTS.md when the section is omitted.
#[test]
fn agent_files_defaults_when_omitted() {
    let config = load_fixture("minimal.toml");
    assert_eq!(config.agent_files.filenames, vec!["CLAUDE.md", "AGENTS.md"]);
}

/// Verifies that the AgentFilesConfig default has the expected filenames.
#[test]
fn agent_files_default_is_valid() {
    let config = AgentFilesConfig::default();
    assert_eq!(config.filenames, vec!["CLAUDE.md", "AGENTS.md"]);
}

/// Tests that custom agent filenames are deserialized from TOML.
#[test]
fn deserialize_agent_files_custom() {
    let config = load_fixture("agent_files_custom.toml");
    assert_eq!(config.agent_files.filenames, vec!["COPILOT.md"]);
}

/// Tests that an empty filenames list is accepted and results in no agent files.
#[test]
fn deserialize_agent_files_empty_filenames() {
    let config = load_fixture("agent_files_empty.toml");
    assert!(config.agent_files.filenames.is_empty());
}

/// Tests that an agent_files section without a filenames key uses defaults.
#[test]
fn deserialize_agent_files_section_without_filenames() {
    let config = load_fixture("agent_files_section_only.toml");
    assert_eq!(config.agent_files.filenames, vec!["CLAUDE.md", "AGENTS.md"]);
}

/// Tests that repository defaults to passthrough strategy when the section is omitted.
#[test]
fn repository_defaults_when_omitted() {
    let config = load_fixture("minimal.toml");
    assert_eq!(config.repository.storage_strategy, StorageStrategy::Passthrough);
}

/// Tests that the hardlink storage strategy deserializes correctly.
#[test]
fn repository_hardlink_strategy() {
    let config = load_fixture("repository_hardlink.toml");
    assert_eq!(config.repository.storage_strategy, StorageStrategy::Hardlink);
}

/// Tests that the snapshot storage strategy deserializes correctly.
#[test]
fn repository_snapshot_strategy() {
    let toml = "[repository]\nstorage_strategy = \"snapshot\"";
    let config: NyneConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.repository.storage_strategy, StorageStrategy::Snapshot);
}

/// Tests that the passthrough storage strategy deserializes correctly.
#[test]
fn repository_passthrough_strategy() {
    let toml = "[repository]\nstorage_strategy = \"passthrough\"";
    let config: NyneConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.repository.storage_strategy, StorageStrategy::Passthrough);
}

/// Tests that sandbox defaults to "nyne-sandbox" hostname when the section is omitted.
#[test]
fn sandbox_defaults_when_omitted() {
    let config = load_fixture("minimal.toml");
    assert_eq!(config.sandbox.hostname, "nyne-sandbox");
}

/// Tests that a custom sandbox hostname is deserialized from TOML.
#[test]
fn sandbox_custom_hostname() {
    let config = load_fixture("sandbox_custom_hostname.toml");
    assert_eq!(config.sandbox.hostname, "my-sandbox");
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

/// Verifies that bind mount flags convert to the correct kernel MountFlags bitset.
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

/// Verifies that a NyneConfig with custom agent filenames passes validation.
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

    let toml_value = serde_json::to_value(&from_toml).expect("serialization of deserialized config");
    let default_value = serde_json::to_value(&from_default).expect("serialization of default config");

    assert_eq!(
        toml_value, default_value,
        "NyneConfig::default() diverges from serde defaults -- \
         update the manual Default impl to match #[serde(default)] attributes"
    );
}

#[derive(Debug, Default, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Dummy {
    #[serde(default)]
    name: String,
}

#[test]
fn deserialize_plugin_config_valid() {
    let value = serde_json::json!({"name": "test"});
    let result: Dummy = deserialize_plugin_config(&value);
    assert_eq!(result.name, "test");
}

#[test]
fn deserialize_plugin_config_falls_back_on_error() {
    // deny_unknown_fields should reject "bogus", but the helper falls back to Default.
    let value = serde_json::json!({"bogus": true});
    let result: Dummy = deserialize_plugin_config(&value);
    assert_eq!(result, Dummy::default());
}

#[test]
fn deserialize_plugin_config_borrows_value() {
    // Null → falls back to Default (empty string), proving it borrows without clone.
    let value = serde_json::Value::Null;
    let result: Dummy = deserialize_plugin_config(&value);
    assert_eq!(result, Dummy::default());
}
