use crate::edit::splice::extend_delete_range;

#[test]
fn no_trailing_blanks() {
    let source = "fn foo() {}\nfn bar() {}\n";
    let span = 0..12; // "fn foo() {}\n"
    let result = extend_delete_range(source, &span);
    assert_eq!(result, 0..12);
}

#[test]
fn one_trailing_blank() {
    let source = "fn foo() {}\n\nfn bar() {}\n";
    let span = 0..12; // "fn foo() {}\n"
    let result = extend_delete_range(source, &span);
    assert_eq!(result, 0..13); // absorbs the "\n"
}

#[test]
fn multiple_trailing_blanks() {
    let source = "fn foo() {}\n\n\n\nfn bar() {}\n";
    let span = 0..12;
    let result = extend_delete_range(source, &span);
    assert_eq!(result, 0..15); // absorbs "\n\n\n"
}

#[test]
fn trailing_blanks_at_eof() {
    let source = "fn foo() {}\n\n\n";
    let span = 0..12;
    let result = extend_delete_range(source, &span);
    assert_eq!(result, 0..14); // absorbs all trailing whitespace
}

#[test]
fn whitespace_only_lines() {
    let source = "fn foo() {}\n  \n\t\nfn bar() {}\n";
    let span = 0..12;
    let result = extend_delete_range(source, &span);
    assert_eq!(result, 0..17); // absorbs "  \n\t\n"
}
