use crate::edit::splice::extend_delete_range;

/// Tests that a range without trailing blank lines is unchanged.
#[test]
fn no_trailing_blanks() {
    let source = "fn foo() {}\nfn bar() {}\n";
    let span = 0..12; // "fn foo() {}\n"
    let result = extend_delete_range(source, &span);
    assert_eq!(result, 0..12);
}

/// Tests that one trailing blank line is absorbed into the deletion range.
#[test]
fn one_trailing_blank() {
    let source = "fn foo() {}\n\nfn bar() {}\n";
    let span = 0..12; // "fn foo() {}\n"
    let result = extend_delete_range(source, &span);
    assert_eq!(result, 0..13); // absorbs the "\n"
}

/// Tests that multiple trailing blank lines are all absorbed.
#[test]
fn multiple_trailing_blanks() {
    let source = "fn foo() {}\n\n\n\nfn bar() {}\n";
    let span = 0..12;
    let result = extend_delete_range(source, &span);
    assert_eq!(result, 0..15); // absorbs "\n\n\n"
}

/// Tests that trailing blank lines at EOF are absorbed.
#[test]
fn trailing_blanks_at_eof() {
    let source = "fn foo() {}\n\n\n";
    let span = 0..12;
    let result = extend_delete_range(source, &span);
    assert_eq!(result, 0..14); // absorbs all trailing whitespace
}

/// Tests that whitespace-only trailing lines are treated as blank.
#[test]
fn whitespace_only_lines() {
    let source = "fn foo() {}\n  \n\t\nfn bar() {}\n";
    let span = 0..12;
    let result = extend_delete_range(source, &span);
    assert_eq!(result, 0..17); // absorbs "  \n\t\n"
}
