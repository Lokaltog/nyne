use rstest::rstest;
use syn::LitStr;

use super::*;

/// Helper to create a `LitStr` from a string value for testing.
fn lit(s: &str) -> LitStr { syn::parse_str::<LitStr>(&format!("\"{s}\"")).expect("valid string literal") }

#[rstest]
#[case("hello", None)]
#[case("{name}", Some((0, 5)))]
#[case("pre-{x}-suf", Some((4, 6)))]
#[case("{name}@", Some((0, 5)))]
#[case("BLAME.md:{spec}", Some((9, 14)))]
fn validate_braces_ok(#[case] input: &str, #[case] expected: Option<(usize, usize)>) {
    assert_eq!(validate_braces(input, &lit(input)).unwrap(), expected);
}

#[rstest]
#[case("{a}{b}", "multiple `{`")]
#[case("{a}}", "multiple `}`")]
#[case("}a{", "before opening")]
#[case("foo{bar", "unclosed")]
#[case("foo}bar", "unopened")]
#[case("{}", "empty capture")]
fn validate_braces_err(#[case] input: &str, #[case] expected_msg: &str) {
    let err = validate_braces(input, &lit(input)).unwrap_err();
    assert!(
        err.to_string().contains(expected_msg),
        "input {input:?}: expected error containing {expected_msg:?}, got: {err}"
    );
}

#[rstest]
#[case("hello", ParsedPattern::Exact("hello".to_owned()))]
#[case("**", ParsedPattern::Glob)]
#[case("{name}", ParsedPattern::Capture { name: "name".to_owned(), prefix: None, suffix: None })]
#[case("BLAME.md:{spec}", ParsedPattern::Capture { name: "spec".to_owned(), prefix: Some("BLAME.md:".to_owned()), suffix: None })]
#[case("{name}@", ParsedPattern::Capture { name: "name".to_owned(), prefix: None, suffix: Some("@".to_owned()) })]
#[case("{..rest}", ParsedPattern::RestCapture { name: "rest".to_owned(), suffix: None })]
fn parse_pattern_ok(#[case] input: &str, #[case] expected: ParsedPattern) {
    assert_eq!(parse_pattern(&lit(input)).unwrap(), expected);
}

#[rstest]
#[case("{a}{b}", "multiple")]
fn parse_pattern_err(#[case] input: &str, #[case] expected_msg: &str) {
    let err = parse_pattern(&lit(input)).unwrap_err();
    assert!(
        err.to_string().contains(expected_msg),
        "input {input:?}: expected error containing {expected_msg:?}, got: {err}"
    );
}
