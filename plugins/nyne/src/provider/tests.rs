use nyne::templates::HandleBuilder;
use rstest::rstest;

use super::views::languages_display;
use super::*;

#[rstest]
#[case::empty(&[], "(none detected)")]
#[case::single(&[("rs", 10)], "rs")]
#[case::multiple(&[("rs", 50), ("toml", 10), ("md", 5)], "rs, toml, md")]
fn languages_display_formatting(#[case] input: &[(&str, usize)], #[case] expected: &str) {
    let owned: Vec<(String, usize)> = input.iter().map(|(s, n)| ((*s).to_owned(), *n)).collect();
    assert_eq!(languages_display(&owned), expected);
}

#[test]
fn render_status_contains_expected_sections() {
    let ctx = Arc::new(nyne::test_support::stub_activation_context());
    let mut b = HandleBuilder::new();
    b.register("test", include_str!("templates/nyne.md.j2"));

    let text = String::from_utf8(render_status(&b.finish(), "test", &ctx, Instant::now())).unwrap();

    assert!(text.contains("Source (host)"), "should contain host source label");
    assert!(text.contains("Source (mount)"), "should contain mount source label");
    assert!(text.contains("/tmp/nyne-test"), "should contain source dir");
    assert!(text.contains("mounted"), "should contain mount state");
    assert!(text.contains("Uptime"), "should contain uptime header");
    assert!(
        text.contains("Middleware Chain"),
        "should contain chain section heading"
    );
}
