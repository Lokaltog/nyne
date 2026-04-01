use std::path::Path;

use lsp_types::{Location, Position, Range, SymbolInformation, SymbolKind, Uri};
use nyne_source::SourcePaths;
use rstest::rstest;

use super::*;

/// Builds a `SymbolInformation` fixture for testing.
fn sym_info(name: &str, path: &str, line: u32) -> SymbolInformation {
    sym_info_uri(name, &format!("file:///source/{path}"), line)
}

/// Builds a `SymbolInformation` fixture with an explicit URI.
fn sym_info_uri(name: &str, uri: &str, line: u32) -> SymbolInformation {
    let uri: Uri = uri.parse().unwrap();
    #[expect(deprecated, reason = "deprecated field required by lsp_types constructor")]
    SymbolInformation {
        name: name.to_owned(),
        kind: SymbolKind::FUNCTION,
        tags: None,
        deprecated: None,
        location: Location {
            uri,
            range: Range::new(Position::new(line, 0), Position::new(line, 10)),
        },
        container_name: None,
    }
}

/// Returns the base VFS path for workspace symbol search queries.
fn search_base() -> PathBuf { PathBuf::from("@/search/symbols/query") }

/// Verifies symlink name and target generation for workspace symbols.
#[rstest]
#[case(
    "process",
    "src/handlers.rs",
    41,
    "handlers.rs::process",
    "src/handlers.rs@/symbols/at-line/42"
)]
#[case(
    "Config",
    "src/config/mod.rs",
    0,
    "mod.rs::Config",
    "src/config/mod.rs@/symbols/at-line/1"
)]
#[case("main", "main.rs", 5, "main.rs::main", "main.rs@/symbols/at-line/6")]
fn symlink_name_and_target(
    #[case] name: &str,
    #[case] path: &str,
    #[case] line: u32,
    #[case] expected_name: &str,
    #[case] expected_target: &str,
) {
    let nodes = build_search_symlinks(
        &[sym_info(name, path, line)],
        Path::new("/source"),
        &search_base(),
        &SourcePaths::default(),
    );

    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].name(), expected_name);
    assert_eq!(nodes[0].kind(), nyne::router::NodeKind::Symlink);
    let target = nodes[0].target().expect("symlink should have a target");
    assert!(
        target.to_str().unwrap().ends_with(expected_target),
        "target {target:?} should end with {expected_target:?}"
    );
}

/// Tests that duplicate symbols with the same name and location are deduplicated.
#[test]
fn deduplicates_by_link_name() {
    let symbols = [sym_info("Foo", "src/a.rs", 10), sym_info("Foo", "src/a.rs", 10)];
    let nodes = build_search_symlinks(&symbols, Path::new("/source"), &search_base(), &SourcePaths::default());
    assert_eq!(nodes.len(), 1);
}

/// Tests that symbols outside the project root are excluded.
#[test]
fn skips_non_project_files() {
    let sym = sym_info_uri("ext", "file:///other/root/lib.rs", 0);
    let nodes = build_search_symlinks(&[sym], Path::new("/source"), &search_base(), &SourcePaths::default());
    assert!(nodes.is_empty());
}

/// Tests that an empty symbol list produces no symlinks.
#[test]
fn empty_results() {
    assert!(build_search_symlinks(&[], Path::new("/source"), &search_base(), &SourcePaths::default()).is_empty());
}

/// Tests that symbols with the same name but different files get unique link names.
#[test]
fn same_name_different_files() {
    let symbols = [sym_info("process", "src/a.rs", 10), sym_info("process", "src/b.rs", 20)];
    let nodes = build_search_symlinks(&symbols, Path::new("/source"), &search_base(), &SourcePaths::default());
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].name(), "a.rs::process");
    assert_eq!(nodes[1].name(), "b.rs::process");
}

/// Tests that workspace symbol symlinks have a zero-TTL cache policy.
#[test]
fn symlinks_have_cache_policy() {
    let nodes = build_search_symlinks(
        &[sym_info("Config", "src/config.rs", 5)],
        Path::new("/source"),
        &search_base(),
        &SourcePaths::default(),
    );
    assert_eq!(nodes.len(), 1);
    assert!(nodes[0].cache_policy().is_some(), "should have a cache policy");
}

/// Verifies that symlink targets use the configured source paths, not hardcoded
/// defaults.
#[rstest]
#[case::default_paths("symbols", "at-line", 41, "src/main.rs@/symbols/at-line/42")]
#[case::custom_suffix("sym_test", "line_test", 41, "src/main.rs@/sym_test/line_test/42")]
#[case::single_char("s", "l", 0, "src/main.rs@/s/l/1")]
fn symlink_targets_use_configured_source_paths(
    #[case] symbols: &str,
    #[case] at_line: &str,
    #[case] lsp_line: u32,
    #[case] expected_suffix: &str,
) {
    let paths = SourcePaths::new(symbols, at_line);
    let nodes = build_search_symlinks(
        &[sym_info("main", "src/main.rs", lsp_line)],
        Path::new("/source"),
        &search_base(),
        &paths,
    );
    assert_eq!(nodes.len(), 1);
    let target = nodes[0].target().expect("symlink should have a target");
    assert!(
        target.to_str().unwrap().ends_with(expected_suffix),
        "target {target:?} should end with {expected_suffix:?}"
    );
}
