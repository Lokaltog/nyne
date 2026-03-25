use rstest::rstest;

use super::*;
use crate::templates::{HandleBuilder, TemplateHandle, serialize_view};

/// Creates a template handle for directory overview tests.
fn overview_handle() -> TemplateHandle {
    let mut b = HandleBuilder::new();
    let key = b.register(TMPL_OVERVIEW, include_str!("templates/overview.md.j2"));
    let engine = b.finish();
    TemplateHandle::new(&engine, key)
}

#[rstest]
#[case::files_and_dirs(
    "src",
    vec![
        FileEntry { name: "main.rs".into(), bytes: 1000, description: "Entry point for the application.".into() },
        FileEntry { name: "lib.rs".into(), bytes: 4800, description: "Crate root — re-exports public API.".into() },
    ],
    vec!["config".into(), "utils".into()],
)]
#[case::files_only(
    "flat",
    vec![
        FileEntry { name: "README.md".into(), bytes: 320, description: String::new() },
    ],
    Vec::new(),
)]
#[case::dirs_only(
    "root",
    Vec::new(),
    vec!["bin".into(), "src".into()],
)]
/// Tests that directory overview templates render correctly for various inputs.
#[case::empty_dir("empty", Vec::new(), Vec::new())]
fn overview_rendering(#[case] dir_name: &str, #[case] files: Vec<FileEntry>, #[case] subdirs: Vec<String>) {
    let h = overview_handle();
    let view = DirOverviewView {
        dir_name: dir_name.into(),
        files,
        subdirs,
    };
    let output = String::from_utf8(h.render_view(&serialize_view(&view)).expect("render")).expect("utf8");
    insta::assert_snapshot!(format!("overview_{dir_name}"), output);
}
