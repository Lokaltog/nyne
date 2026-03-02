use rstest::rstest;

use super::*;

/// Load a fixture TOML file and deserialize into `NyneConfig`.
fn load_fixture(name: &str) -> NyneConfig {
    let content = crate::test_support::load_fixture("config", name);
    toml::from_str(&content).unwrap_or_else(|e| panic!("failed to parse fixture {name}: {e}"))
}

#[test]
fn default_config_is_valid() {
    let config = NyneConfig::default();
    config.validate().expect("default config should be valid");
    insta::assert_toml_snapshot!(config);
}

#[test]
fn deserialize_minimal_config() {
    let config = load_fixture("minimal.toml");
    insta::assert_toml_snapshot!(config);
}

#[test]
fn deserialize_mount_config() {
    let config = load_fixture("mount.toml");
    insta::assert_toml_snapshot!(config);
}

#[test]
fn deserialize_mount_config_with_exclusions() {
    let config = load_fixture("mount_with_exclusions.toml");
    let mount = config.mount.expect("mount should be present");
    assert_eq!(mount.excluded_patterns, vec!["target", "*.o", ".git"]);
}

#[test]
fn lsp_defaults_when_omitted() {
    let config = load_fixture("minimal.toml");
    insta::assert_toml_snapshot!(config.lsp);
}

#[test]
fn lsp_defaults_with_empty_section() {
    let config = load_fixture("lsp_empty_section.toml");
    assert!(config.lsp.enabled);
    assert_eq!(config.lsp.cache_ttl, std::time::Duration::from_secs(300));
    assert_eq!(config.lsp.diagnostics_timeout, std::time::Duration::from_secs(2));
}

#[test]
fn lsp_disabled() {
    let config = load_fixture("lsp_disabled.toml");
    assert!(!config.lsp.enabled);
}

#[test]
fn lsp_custom_durations() {
    let config = load_fixture("lsp_custom_durations.toml");
    assert_eq!(config.lsp.cache_ttl, std::time::Duration::from_secs(600));
    assert_eq!(config.lsp.diagnostics_timeout, std::time::Duration::from_secs(5));
}

#[test]
fn lsp_server_override_args() {
    let config = load_fixture("lsp_server_override_args.toml");
    let ra = &config.lsp.servers["rust-analyzer"];
    assert!(ra.enabled);
    assert!(ra.command.is_none());
    assert_eq!(
        ra.args.as_deref(),
        Some(&["--log-file".to_owned(), "/tmp/ra.log".to_owned()][..])
    );
}

#[test]
fn lsp_server_override_command() {
    let config = load_fixture("lsp_server_override_command.toml");
    let pyright = &config.lsp.servers["pyright"];
    assert!(pyright.enabled);
    assert_eq!(pyright.command.as_deref(), Some("basedpyright-langserver"));
    assert!(pyright.args.is_none());
}

#[test]
fn lsp_server_disable() {
    let config = load_fixture("lsp_server_disable.toml");
    assert!(!config.lsp.servers["basedpyright"].enabled);
}

#[test]
fn lsp_custom_server() {
    let config = load_fixture("lsp_custom_server.toml");
    insta::assert_debug_snapshot!(config.lsp.custom);
}

#[test]
fn lsp_multiple_custom_servers() {
    let config = load_fixture("lsp_multiple_custom_servers.toml");
    insta::assert_debug_snapshot!(config.lsp.custom);
}

#[test]
fn lsp_custom_server_no_args() {
    let config = load_fixture("lsp_custom_server_no_args.toml");
    assert!(config.lsp.custom[0].args.is_empty());
}

#[test]
fn lsp_full_config() {
    let config = load_fixture("lsp_full.toml");
    config.validate().expect("full lsp config should be valid");
    assert!(config.lsp.enabled);
    assert_eq!(config.lsp.cache_ttl, std::time::Duration::from_secs(600));
    assert_eq!(config.lsp.diagnostics_timeout, std::time::Duration::from_secs(3));
    assert_eq!(config.lsp.servers.len(), 2);
    assert!(!config.lsp.servers["basedpyright"].enabled);
    assert_eq!(config.lsp.custom.len(), 1);
}

#[rstest]
#[case::unknown_top_level("unknown_key = \"value\"")]
#[case::unknown_mount("[mount]\nsource_dir = \"/tmp/src\"\nmountpoint = \"/tmp/mnt\"\nbogus = true")]
#[case::unknown_lsp("[lsp]\nbogus = true")]
#[case::unknown_lsp_server_override("[lsp.servers.rust-analyzer]\nbogus = true")]
#[case::unknown_lsp_custom("[[lsp.custom]]\nname = \"foo\"\ncommand = \"foo\"\nextensions = [\"bar\"]\nbogus = true")]
#[case::unknown_repository("[repository]\nbogus = true")]
#[case::unknown_sandbox("[sandbox]\nbogus = true")]
#[case::unknown_bind_mount("[[sandbox.bind_mounts]]\nsource = \"/src\"\ntarget = \"/dst\"\nbogus = true")]
#[case::invalid_bind_mount_flag("[[sandbox.bind_mounts]]\nsource = \"/src\"\ntarget = \"/dst\"\nflags = [\"bogus\"]")]
#[case::unknown_agent_files("[agent_files]\nbogus = true")]
fn reject_invalid_config(#[case] toml_input: &str) {
    let result: std::result::Result<NyneConfig, _> = toml::from_str(toml_input);
    assert!(result.is_err(), "invalid config should be rejected: {toml_input}");
}

#[test]
fn agent_files_defaults_when_omitted() {
    let config = load_fixture("minimal.toml");
    assert_eq!(config.agent_files.filenames, vec!["CLAUDE.md", "AGENTS.md"]);
}

#[test]
fn agent_files_default_is_valid() {
    let config = AgentFilesConfig::default();
    assert_eq!(config.filenames, vec!["CLAUDE.md", "AGENTS.md"]);
}

#[test]
fn deserialize_agent_files_custom() {
    let config = load_fixture("agent_files_custom.toml");
    assert_eq!(config.agent_files.filenames, vec!["COPILOT.md"]);
}

#[test]
fn deserialize_agent_files_empty_filenames() {
    let config = load_fixture("agent_files_empty.toml");
    assert!(config.agent_files.filenames.is_empty());
}

#[test]
fn deserialize_agent_files_section_without_filenames() {
    let config = load_fixture("agent_files_section_only.toml");
    assert_eq!(config.agent_files.filenames, vec!["CLAUDE.md", "AGENTS.md"]);
}

#[test]
fn repository_defaults_when_omitted() {
    let config = load_fixture("minimal.toml");
    assert_eq!(config.repository.storage_strategy, StorageStrategy::Passthrough);
}

#[test]
fn repository_hardlink_strategy() {
    let config = load_fixture("repository_hardlink.toml");
    assert_eq!(config.repository.storage_strategy, StorageStrategy::Hardlink);
}

#[test]
fn repository_snapshot_strategy() {
    let toml = "[repository]\nstorage_strategy = \"snapshot\"";
    let config: NyneConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.repository.storage_strategy, StorageStrategy::Snapshot);
}

#[test]
fn repository_passthrough_strategy() {
    let toml = "[repository]\nstorage_strategy = \"passthrough\"";
    let config: NyneConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.repository.storage_strategy, StorageStrategy::Passthrough);
}

#[test]
fn sandbox_defaults_when_omitted() {
    let config = load_fixture("minimal.toml");
    assert_eq!(config.sandbox.hostname, "nyne-sandbox");
}

#[test]
fn sandbox_custom_hostname() {
    let config = load_fixture("sandbox_custom_hostname.toml");
    assert_eq!(config.sandbox.hostname, "my-sandbox");
}

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

#[test]
fn sandbox_bind_mounts_mount_flags() {
    use rustix::mount::MountFlags;

    let config = load_fixture("sandbox_bind_mounts.toml");

    let first_flags = config.sandbox.bind_mounts[0].mount_flags();
    assert_eq!(first_flags, Some(MountFlags::RDONLY | MountFlags::NOEXEC));

    let second_flags = config.sandbox.bind_mounts[1].mount_flags();
    assert_eq!(second_flags, None);
}

#[test]
fn sandbox_env() {
    let config = load_fixture("sandbox_env.toml");
    assert_eq!(config.sandbox.env.len(), 2);
    assert_eq!(config.sandbox.env["MY_CUSTOM_VAR"], "hello");
    assert_eq!(config.sandbox.env["ANOTHER_VAR"], "world");
}

#[test]
fn sandbox_env_defaults_empty() {
    let config = load_fixture("minimal.toml");
    assert!(config.sandbox.env.is_empty());
}

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
