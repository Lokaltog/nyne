use rstest::rstest;

use super::*;

/// Verifies that sanitize produces valid kebab-case session ID components.
#[rstest]
#[case("nyne", "nyne")]
#[case("My_Project", "my-project")]
#[case("foo--bar", "foo-bar")]
#[case("-leading-", "leading")]
#[case("UPPER", "upper")]
#[case("a.b.c", "a-b-c")]
#[case("---", "")]
#[case("", "")]
#[case("123", "123")]
fn sanitize_cases(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(sanitize(input), expected);
}

/// Verifies that `SessionId::new` accepts valid IDs and rejects invalid ones.
#[rstest]
#[case("valid-id", true)]
#[case("123", true)]
#[case("a", true)]
#[case("", false)]
#[case("-leading", false)]
#[case("UPPER", false)]
#[case("has space", false)]
fn session_id_validation(#[case] input: &str, #[case] valid: bool) {
    assert_eq!(SessionId::new(input.into()).is_ok(), valid);
}
