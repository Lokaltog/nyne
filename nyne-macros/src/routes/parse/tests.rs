use syn::LitStr;

use super::*;

/// Helper to create a `LitStr` from a string value for testing.
fn lit(s: &str) -> LitStr { syn::parse_str::<LitStr>(&format!("\"{s}\"")).expect("valid string literal") }

#[test]
fn no_braces_returns_none() {
    assert_eq!(validate_braces("hello", &lit("hello")).unwrap(), None);
}

#[test]
fn single_capture_returns_positions() {
    assert_eq!(validate_braces("{name}", &lit("{name}")).unwrap(), Some((0, 5)));
}

#[test]
fn capture_with_prefix_and_suffix() {
    assert_eq!(
        validate_braces("pre-{x}-suf", &lit("pre-{x}-suf")).unwrap(),
        Some((4, 6))
    );
}

#[test]
fn multiple_open_braces_rejected() {
    assert!(
        validate_braces("{a}{b}", &lit("{a}{b}"))
            .unwrap_err()
            .to_string()
            .contains("multiple `{`"),
    );
}

#[test]
fn multiple_close_braces_rejected() {
    assert!(
        validate_braces("{a}}", &lit("{a}}"))
            .unwrap_err()
            .to_string()
            .contains("multiple `}`"),
    );
}

#[test]
fn reversed_braces_rejected() {
    assert!(
        validate_braces("}a{", &lit("}a{"))
            .unwrap_err()
            .to_string()
            .contains("before opening"),
    );
}

#[test]
fn unclosed_brace_rejected() {
    assert!(
        validate_braces("foo{bar", &lit("foo{bar"))
            .unwrap_err()
            .to_string()
            .contains("unclosed"),
    );
}

#[test]
fn unopened_brace_rejected() {
    assert!(
        validate_braces("foo}bar", &lit("foo}bar"))
            .unwrap_err()
            .to_string()
            .contains("unopened"),
    );
}

#[test]
fn empty_capture_rejected() {
    assert!(
        validate_braces("{}", &lit("{}"))
            .unwrap_err()
            .to_string()
            .contains("empty capture"),
    );
}

#[test]
fn exact_literal() {
    assert_eq!(
        parse_pattern(&lit("hello")).unwrap(),
        ParsedPattern::Exact("hello".to_owned())
    );
}

#[test]
fn glob_pattern() {
    assert_eq!(parse_pattern(&lit("**")).unwrap(), ParsedPattern::Glob);
}

#[test]
fn simple_capture() {
    assert_eq!(parse_pattern(&lit("{name}")).unwrap(), ParsedPattern::Capture {
        name: "name".to_owned(),
        prefix: None,
        suffix: None,
    });
}

#[test]
fn capture_with_prefix() {
    assert_eq!(
        parse_pattern(&lit("BLAME.md:{spec}")).unwrap(),
        ParsedPattern::Capture {
            name: "spec".to_owned(),
            prefix: Some("BLAME.md:".to_owned()),
            suffix: None,
        }
    );
}

#[test]
fn capture_with_suffix() {
    assert_eq!(parse_pattern(&lit("{name}@")).unwrap(), ParsedPattern::Capture {
        name: "name".to_owned(),
        prefix: None,
        suffix: Some("@".to_owned()),
    });
}

#[test]
fn rest_capture() {
    assert_eq!(parse_pattern(&lit("{..rest}")).unwrap(), ParsedPattern::RestCapture {
        name: "rest".to_owned(),
        suffix: None,
    });
}

#[test]
fn multi_capture_rejected_by_parse_pattern() {
    assert!(
        parse_pattern(&lit("{a}{b}"))
            .unwrap_err()
            .to_string()
            .contains("multiple"),
    );
}
