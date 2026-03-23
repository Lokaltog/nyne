use std::path::Path;

use lsp_types::{Location, Position, Range, SymbolInformation, SymbolKind, Uri};
use nyne::types::vfs_path::VfsPath;
use rstest::rstest;

use super::*;

fn sym_info(name: &str, path: &str, line: u32) -> SymbolInformation {
    let uri: Uri = format!("file:///overlay/{path}").parse().unwrap();
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

fn base() -> VfsPath { VfsPath::new("@/search/symbols/query").unwrap() }

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
    let nodes = build_symlinks(&[sym_info(name, path, line)], Path::new("/overlay"), &base());

    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].name(), expected_name);
    let nyne::node::kind::NodeKind::Symlink { target } = nodes[0].kind() else {
        panic!("expected symlink node");
    };
    // Target is relative from @/search/symbols/query → project root.
    assert!(
        target.to_str().unwrap().ends_with(expected_target),
        "target {target:?} should end with {expected_target:?}"
    );
}

#[test]
fn deduplicates_by_link_name() {
    let symbols = [sym_info("Foo", "src/a.rs", 10), sym_info("Foo", "src/a.rs", 10)];

    let nodes = build_symlinks(&symbols, Path::new("/overlay"), &base());

    assert_eq!(nodes.len(), 1);
}

#[test]
fn skips_non_project_files() {
    let uri: lsp_types::Uri = "file:///other/root/lib.rs".parse().unwrap();
    #[expect(deprecated, reason = "deprecated field required by lsp_types constructor")]
    let sym = SymbolInformation {
        name: "ext".to_owned(),
        kind: SymbolKind::FUNCTION,
        tags: None,
        deprecated: None,
        location: Location {
            uri,
            range: Range::new(Position::new(0, 0), Position::new(0, 10)),
        },
        container_name: None,
    };

    let nodes = build_symlinks(&[sym], Path::new("/overlay"), &base());
    assert!(nodes.is_empty());
}

#[test]
fn empty_results() {
    assert!(build_symlinks(&[], Path::new("/overlay"), &base()).is_empty());
}

#[test]
fn same_name_different_files() {
    let symbols = [sym_info("process", "src/a.rs", 10), sym_info("process", "src/b.rs", 20)];

    let nodes = build_symlinks(&symbols, Path::new("/overlay"), &base());

    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].name(), "a.rs::process");
    assert_eq!(nodes[1].name(), "b.rs::process");
}
