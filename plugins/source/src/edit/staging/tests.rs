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
