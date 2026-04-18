use rstest::rstest;

use super::*;

#[test]
fn new_staging_is_empty() {
    let staging = EditStaging::new();
    assert!(staging.is_empty());
    assert!(staging.snapshot().is_empty());
}

#[test]
fn stage_returns_incrementing_sequence_numbers() {
    let staging = EditStaging::new();
    let seq0 = staging.stage(
        PathBuf::from("a.rs"),
        vec!["Foo".into()],
        EditOpKind::Replace,
        Some("fn foo() {}".into()),
    );
    let seq1 = staging.stage(PathBuf::from("b.rs"), vec!["Bar".into()], EditOpKind::Delete, None);
    assert_eq!(seq0, 0);
    assert_eq!(seq1, 1);
    assert!(!staging.is_empty());
}

#[test]
fn snapshot_preserves_staged_ops() {
    let staging = EditStaging::new();
    staging.stage(
        PathBuf::from("a.rs"),
        vec!["Foo".into()],
        EditOpKind::Replace,
        Some("new body".into()),
    );
    staging.stage(PathBuf::from("a.rs"), vec!["Bar".into()], EditOpKind::Delete, None);

    let snap = staging.snapshot();
    assert_eq!(snap.len(), 1); // one file
    assert_eq!(snap[&PathBuf::from("a.rs")].len(), 2); // two ops
    assert!(!staging.is_empty()); // snapshot doesn't drain
}

#[test]
fn drain_empties_staging() {
    let staging = EditStaging::new();
    staging.stage(PathBuf::from("a.rs"), vec!["Foo".into()], EditOpKind::Delete, None);

    let drained = staging.drain();
    assert_eq!(drained.len(), 1);
    assert!(staging.is_empty());
}

#[test]
fn drain_file_removes_only_scoped_entries() {
    let staging = EditStaging::new();
    staging.stage(PathBuf::from("a.rs"), vec!["Foo".into()], EditOpKind::Delete, None);
    staging.stage(
        PathBuf::from("b.rs"),
        vec!["Bar".into()],
        EditOpKind::Replace,
        Some("x".into()),
    );

    let drained = staging.drain_file(&PathBuf::from("a.rs"));
    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0].1.kind, EditOpKind::Delete);

    let remaining = staging.snapshot();
    assert_eq!(remaining.len(), 1);
    assert!(remaining.contains_key(&PathBuf::from("b.rs")));
}

#[test]
fn drain_file_missing_is_noop() {
    let staging = EditStaging::new();
    staging.stage(PathBuf::from("a.rs"), vec!["Foo".into()], EditOpKind::Delete, None);

    let drained = staging.drain_file(&PathBuf::from("nonexistent.rs"));
    assert!(drained.is_empty());
    assert_eq!(staging.snapshot().len(), 1);
}

#[rstest]
#[case::scoped(Some(PathBuf::from("a.rs")))]
#[case::mount_wide(None)]
fn staged_diff_node_attaches_writable(#[case] scope: Option<PathBuf>) {
    let staging = EditStaging::new();
    let node = staging.staged_diff_node(scope, "staged.diff");
    assert_eq!(node.name(), "staged.diff");
    // Writable is attached in both scopes — inspectable via the node's capability.
    assert!(node.writable().is_some(), "staged.diff node must carry a Writable");
}

#[rstest]
#[case::scoped_a(Some(PathBuf::from("a.rs")), &["a.rs"])]
#[case::scoped_b(Some(PathBuf::from("b.rs")), &["b.rs"])]
#[case::scoped_missing(Some(PathBuf::from("nonexistent.rs")), &[])]
#[case::mount_wide(None, &["a.rs", "b.rs"])]
fn scoped_snapshot_filters_by_scope(#[case] scope: Option<PathBuf>, #[case] expected_files: &[&str]) {
    use std::sync::Arc;

    use nyne::router::MemFs;

    use crate::syntax::SyntaxRegistry;
    use crate::syntax::decomposed::DecompositionCache;

    let staging = EditStaging::new();
    staging.stage(PathBuf::from("a.rs"), vec!["Foo".into()], EditOpKind::Delete, None);
    staging.stage(
        PathBuf::from("b.rs"),
        vec!["Bar".into()],
        EditOpKind::Replace,
        Some("x".into()),
    );

    let registry = Arc::new(SyntaxRegistry::build());
    let decomposition = DecompositionCache::new(Arc::new(MemFs::default()), Arc::clone(&registry), 8);
    let action = BatchEditAction::new(staging, decomposition, registry, scope);

    let mut visible: Vec<String> = action
        .scoped_snapshot()
        .keys()
        .map(|p| p.display().to_string())
        .collect();
    visible.sort();
    let expected: Vec<String> = expected_files.iter().map(|s| (*s).into()).collect();
    assert_eq!(visible, expected);
}

#[test]
fn clear_discards_all_ops() {
    let staging = EditStaging::new();
    staging.stage(PathBuf::from("a.rs"), vec!["Foo".into()], EditOpKind::Delete, None);
    staging.clear();
    assert!(staging.is_empty());
}

#[rstest]
#[case::replace(EditOpKind::Replace, Some("body".into()))]
#[case::delete(EditOpKind::Delete, None)]
#[case::insert_before(EditOpKind::InsertBefore, Some("above".into()))]
#[case::insert_after(EditOpKind::InsertAfter, Some("below".into()))]
#[case::append(EditOpKind::Append, Some("child".into()))]
fn stage_all_op_kinds(#[case] kind: EditOpKind, #[case] content: Option<String>) {
    let staging = EditStaging::new();
    staging.stage(PathBuf::from("a.rs"), vec!["Foo".into()], kind, content);

    let snap = staging.snapshot();
    let ops = &snap[&PathBuf::from("a.rs")];
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].1.kind, kind);
}

#[test]
fn cross_file_staging() {
    let staging = EditStaging::new();
    staging.stage(
        PathBuf::from("a.rs"),
        vec!["Foo".into()],
        EditOpKind::Replace,
        Some("new a".into()),
    );
    staging.stage(PathBuf::from("b.rs"), vec!["Bar".into()], EditOpKind::Delete, None);

    let snap = staging.snapshot();
    assert_eq!(snap.len(), 2);
    assert!(snap.contains_key(&PathBuf::from("a.rs")));
    assert!(snap.contains_key(&PathBuf::from("b.rs")));
}
