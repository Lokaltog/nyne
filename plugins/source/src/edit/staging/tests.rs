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

#[test]
fn staged_diff_node_carries_scope() {
    let staging = EditStaging::new();
    let scope = PathBuf::from("a.rs");

    let scoped = staging.staged_diff_node(Some(scope.clone()), "staged.diff");
    assert_eq!(scoped.name(), "staged.diff");

    let unscoped = staging.staged_diff_node(None, "staged.diff");
    assert_eq!(unscoped.name(), "staged.diff");
    // Writable is attached in both cases — inspectable via the node's capability.
    assert!(scoped.writable().is_some());
    assert!(unscoped.writable().is_some());
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
