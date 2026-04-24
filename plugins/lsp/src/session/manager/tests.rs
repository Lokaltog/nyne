use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use nyne::process::Spawner;
use nyne_source::SyntaxRegistry;
use rstest::rstest;

use crate::plugin::config::Config;
use crate::session::Registry;
use crate::session::manager::Manager;

/// Build a manager with the given config pointing at a non-existent root.
///
/// Useful for testing gating/routing logic without spawning real servers.
fn test_manager_with_config(config: Config) -> Manager {
    let registry = Registry::build_with_config(&config);
    let syntax = SyntaxRegistry::global();
    let spawner = Arc::new(Spawner::new());

    let path_resolver =
        crate::session::path::PathResolver::new(PathBuf::from("/nonexistent"), PathBuf::from("/nonexistent"));
    Manager::new(registry, syntax, config, spawner, HashMap::new(), path_resolver)
}

/// Build a manager with default config (enabled/disabled) pointing at a
/// non-existent root.
fn test_manager(enabled: bool) -> Manager {
    let mut config = Config::default();
    config.enabled = enabled;
    test_manager_with_config(config)
}

/// Verifies that no client is returned across disabled / unknown-extension /
/// missing-project-root paths. "rs" has a registered syntax and LSP server,
/// but the detect function checks for Cargo.toml in /nonexistent — detection
/// fails and no spawn is attempted.
#[rstest]
#[case::disabled(false, "rs")]
#[case::unknown_extension(true, "xyz")]
#[case::missing_project_root(true, "rs")]
fn no_client_returned(#[case] enabled: bool, #[case] ext: &str) {
    let mgr = test_manager(enabled);
    assert_eq!(mgr.is_enabled(), enabled);
    assert!(mgr.client_for_ext(ext).is_none());
    assert!(mgr.all_clients_for_ext(ext).is_empty());
}

/// Tests that `has_lsp_support` requires both enabled config and syntax registration.
#[rstest]
fn has_lsp_support_requires_enabled_and_syntax() {
    let enabled = test_manager(true);
    let disabled = test_manager(false);

    // "rs" has both syntax and LSP registration.
    assert!(enabled.has_lsp_support("rs"));

    // Disabled config gates everything.
    assert!(!disabled.has_lsp_support("rs"));

    // Unknown extension has no syntax.
    assert!(!enabled.has_lsp_support("xyz"));
}

/// Verifies that the cache is accessible and starts empty.
#[rstest]
fn cache_is_wired() {
    let mgr = test_manager(true);
    assert!(mgr.cache().is_empty());
}

/// Tests that invalidating a file on an empty cache is a no-op.
#[rstest]
fn invalidate_file_delegates_to_cache() {
    let mgr = test_manager(true);
    // Should not panic — just a no-op on empty cache.
    mgr.invalidate_file(std::path::Path::new("/some/file.rs"));
    assert!(mgr.cache().is_empty());
}

/// Verifies that status is empty when no LSP clients are running.
#[rstest]
fn status_empty_when_no_clients() {
    let mgr = test_manager(true);
    assert!(mgr.status().is_empty());
}

/// Tests that the diagnostics timeout comes from config defaults.
#[rstest]
fn diagnostics_timeout_from_config() {
    let mgr = test_manager(true);
    assert_eq!(mgr.diagnostics_timeout(), std::time::Duration::from_secs(2));
}

/// Tests that closing a non-tracked document does not panic.
#[rstest]
fn close_document_noop_when_not_open() {
    let mgr = test_manager(true);
    // Should not panic — no-op when no document is tracked.
    mgr.close_document(std::path::Path::new("/some/file.rs"));
}

// File rename

/// `will_rename_file` must not panic when no LSP client is available.
#[rstest::rstest]
#[case::disabled("rs", false)]
#[case::unknown_ext("xyz", true)]
fn will_rename_file_noop(#[case] ext: &str, #[case] enabled: bool) {
    let mgr = test_manager(enabled);
    let old = PathBuf::from(format!("/nonexistent/src/foo.{ext}"));
    let new = PathBuf::from(format!("/nonexistent/src/bar.{ext}"));
    mgr.will_rename_file(&old, &new);
}

/// `did_rename_file` must not panic when no LSP client is available.
#[rstest::rstest]
#[case::disabled("rs", false)]
#[case::unknown_ext("xyz", true)]
fn did_rename_file_noop(#[case] ext: &str, #[case] enabled: bool) {
    let mgr = test_manager(enabled);
    let old = PathBuf::from(format!("/nonexistent/src/foo.{ext}"));
    let new = PathBuf::from(format!("/nonexistent/src/bar.{ext}"));
    mgr.did_rename_file(&old, &new);
}

/// `resolve_rename_uris` returns `None` when no server can be resolved.
#[rstest::rstest]
#[case::no_server("rs")]
#[case::unknown_ext("xyz")]
fn resolve_rename_uris_none(#[case] ext: &str) {
    let mgr = test_manager(true);
    let old = PathBuf::from(format!("/nonexistent/src/foo.{ext}"));
    let new = PathBuf::from(format!("/nonexistent/src/bar.{ext}"));
    assert!(mgr.resolve_rename_uris(&old, &new).is_none());
}
