use rstest::rstest;

use super::*;

#[derive(Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Dummy {
    #[serde(default)]
    name: String,
}

#[rstest]
#[case::valid(toml::toml! { name = "test" }.into(), Dummy { name: "test".into() })]
#[case::unknown_field_falls_back(toml::toml! { bogus = true }.into(), Dummy::default())]
#[case::non_table_falls_back(toml::Value::Boolean(false), Dummy::default())]
fn from_section_deserializes_or_falls_back(#[case] value: toml::Value, #[case] expected: Dummy) {
    assert_eq!(Dummy::from_section(Some(&value)), expected);
}

#[rstest]
#[case::none_returns_default(None, Dummy::default())]
#[case::valid_section(
    Some(toml::toml! { name = "hello" }.into()),
    Dummy { name: "hello".into() },
)]
#[case::invalid_section_falls_back(
    Some(toml::toml! { bogus = true }.into()),
    Dummy::default(),
)]
fn plugin_config_from_section(#[case] section: Option<toml::Value>, #[case] expected: Dummy) {
    assert_eq!(Dummy::from_section(section.as_ref()), expected);
}
