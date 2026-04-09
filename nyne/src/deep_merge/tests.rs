use rstest::rstest;
use serde_json::{Value as JsonValue, json};

use super::deep_merge;

/// Verifies recursive JSON deep merging across various value types.
#[rstest]
#[case::scalars_replaced(
    json!({"a": 1}),
    json!({"a": 2}),
    json!({"a": 2}),
)]
#[case::nested_objects_merge(
    json!({"env": {"A": "1", "B": "2"}}),
    json!({"env": {"B": "override", "C": "3"}}),
    json!({"env": {"A": "1", "B": "override", "C": "3"}}),
)]
#[case::new_keys_added(
    json!({"a": 1}),
    json!({"b": 2}),
    json!({"a": 1, "b": 2}),
)]
#[case::deeply_nested(
    json!({"a": {"b": {"c": 1, "d": 2}}}),
    json!({"a": {"b": {"c": 99, "e": 3}}}),
    json!({"a": {"b": {"c": 99, "d": 2, "e": 3}}}),
)]
#[case::object_replaced_by_scalar(
    json!({"a": {"nested": true}}),
    json!({"a": "flat"}),
    json!({"a": "flat"}),
)]
#[case::string_arrays(
    json!({"tags": ["a", "b"]}),
    json!({"tags": ["c"]}),
    json!({"tags": ["a", "b", "c"]}),
)]
#[case::number_arrays(
    json!({"ids": [1, 2]}),
    json!({"ids": [3, 4]}),
    json!({"ids": [1, 2, 3, 4]}),
)]
#[case::object_arrays(
    json!({"servers": [{"name": "a", "port": 80}]}),
    json!({"servers": [{"name": "b", "port": 443}]}),
    json!({"servers": [{"name": "a", "port": 80}, {"name": "b", "port": 443}]}),
)]
#[case::nested_arrays(
    json!({"config": {"items": [1]}}),
    json!({"config": {"items": [2]}}),
    json!({"config": {"items": [1, 2]}}),
)]
#[case::overlay_adds_new_array_key(
    json!({"a": [1]}),
    json!({"b": [2]}),
    json!({"a": [1], "b": [2]}),
)]
#[case::empty_base_array(
    json!({"x": []}),
    json!({"x": [1, 2]}),
    json!({"x": [1, 2]}),
)]
#[case::empty_overlay_array(
    json!({"x": [1, 2]}),
    json!({"x": []}),
    json!({"x": [1, 2]}),
)]
#[case::scalar_still_replaced(
    json!({"name": "old"}),
    json!({"name": "new"}),
    json!({"name": "new"}),
)]
#[case::objects_still_deep_merged(
    json!({"a": {"b": 1, "c": 2}}),
    json!({"a": {"b": 3}}),
    json!({"a": {"b": 3, "c": 2}}),
)]
fn json_deep_merge(#[case] mut base: JsonValue, #[case] overlay: JsonValue, #[case] expected: JsonValue) {
    deep_merge(&mut base, &overlay);
    assert_eq!(base, expected);
}

/// Helper to build a `toml::Value` from a TOML literal.
macro_rules! toml_value {
    ($($toml:tt)*) => {
        toml::Value::from(toml::toml! { $($toml)* })
    };
}

#[test]
fn toml_scalar_overwrite() {
    let mut base = toml_value! { key = "base" };
    let overlay = toml_value! { key = "overlay" };
    deep_merge(&mut base, &overlay);
    assert_eq!(base["key"].as_str(), Some("overlay"));
}

#[test]
fn toml_nested_table_merge() {
    let mut base = toml_value! {
        [section]
        a = 1
        b = 2
    };
    let overlay = toml_value! {
        [section]
        b = 99
        c = 3
    };
    deep_merge(&mut base, &overlay);
    let section = base["section"].as_table().unwrap();
    assert_eq!(section["a"].as_integer(), Some(1));
    assert_eq!(section["b"].as_integer(), Some(99));
    assert_eq!(section["c"].as_integer(), Some(3));
}

#[test]
fn toml_array_extended() {
    let mut base = toml_value! { tags = ["a", "b"] };
    let overlay = toml_value! { tags = ["c"] };
    deep_merge(&mut base, &overlay);
    let tags: Vec<&str> = base["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(tags, vec!["a", "b", "c"]);
}

#[test]
fn toml_new_keys_added() {
    let mut base = toml_value! { existing = true };
    let overlay = toml_value! { new_key = "hello" };
    deep_merge(&mut base, &overlay);
    assert_eq!(base["existing"].as_bool(), Some(true));
    assert_eq!(base["new_key"].as_str(), Some("hello"));
}

#[test]
fn toml_deeply_nested_merge() {
    let mut base = toml_value! {
        [a.b.c]
        value = 1
    };
    let overlay = toml_value! {
        [a.b.c]
        value = 2
        extra = true
    };
    deep_merge(&mut base, &overlay);
    assert_eq!(base["a"]["b"]["c"]["value"].as_integer(), Some(2));
    assert_eq!(base["a"]["b"]["c"]["extra"].as_bool(), Some(true));
}
