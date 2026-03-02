use super::*;

#[test]
fn strip_tag_prefix_colon() {
    assert_eq!(strip_tag_prefix("TODO: fix this", "TODO"), Some("fix this".to_owned()));
}

#[test]
fn strip_tag_prefix_paren() {
    assert_eq!(
        strip_tag_prefix("TODO(user): fix this", "TODO"),
        Some("fix this".to_owned())
    );
}

#[test]
fn strip_tag_prefix_dash_rejected() {
    assert_eq!(strip_tag_prefix("FIXME - urgent", "FIXME"), None);
}

#[test]
fn strip_tag_prefix_space_rejected() {
    assert_eq!(strip_tag_prefix("HACK use a real parser", "HACK"), None);
}

#[test]
fn strip_tag_prefix_bare_colon() {
    assert_eq!(strip_tag_prefix("TODO:", "TODO"), Some(String::new()));
}

#[test]
fn byte_to_line_basic() {
    let source = "line0\nline1\nline2\n";
    let starts = build_line_starts(source);
    assert_eq!(byte_to_line(&starts, 0), 0);
    assert_eq!(byte_to_line(&starts, 5), 0); // newline char
    assert_eq!(byte_to_line(&starts, 6), 1); // start of line1
    assert_eq!(byte_to_line(&starts, 12), 2); // start of line2
}

#[test]
fn find_comment_block_line_comments() {
    let source = "fn foo() {}\n// TODO: first\n// continuation\nfn bar() {}\n";
    let line_starts = build_line_starts(source);
    let todo_offset = source.find("TODO").unwrap();
    let block = find_comment_block(source, &line_starts, todo_offset).unwrap();
    let block_text = &source[block];
    assert!(block_text.contains("TODO: first"));
    assert!(block_text.contains("continuation"));
}

#[test]
fn find_comment_block_block_comment() {
    let source = "/* TODO: fix\n   this thing */\nfn foo() {}\n";
    let line_starts = build_line_starts(source);
    let todo_offset = source.find("TODO").unwrap();
    let block = find_comment_block(source, &line_starts, todo_offset).unwrap();
    let block_text = &source[block];
    assert!(block_text.starts_with("/*"));
    assert!(block_text.ends_with("*/"));
}
