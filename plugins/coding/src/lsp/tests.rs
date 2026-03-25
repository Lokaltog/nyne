use rstest::rstest;

use super::*;

#[rstest]
#[case::ts("ts", "typescript")]
#[case::tsx("tsx", "typescriptreact")]
#[case::rust("rs", "rust")]
#[case::python("py", "python")]
fn language_id_for_extension(#[case] ext: &str, #[case] expected: &str) {
    let registry = LspRegistry::build();
    assert_eq!(
        registry.language_id_for(ext),
        Some(expected),
        "language_id_for({ext:?}) should be {expected:?}",
    );
}
