use rstest::rstest;
use serde_json::{Value, json};

use super::{deep_merge, deep_merge_non_null};

/// Verifies recursive JSON object merging across various value types.
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
fn deep_merge_cases(#[case] mut base: Value, #[case] overlay: Value, #[case] expected: Value) {
    deep_merge(&mut base, &overlay);
    assert_eq!(base, expected);
}

/// Verifies that `deep_merge_non_null` skips null overlay values.
#[rstest]
#[case::null_skipped(
    json!({"a": 1, "b": 2}),
    json!({"a": null, "b": 3}),
    json!({"a": 1, "b": 3}),
)]
#[case::nested_null_skipped(
    json!({"env": {"A": "1", "B": "2"}}),
    json!({"env": {"A": null, "C": "3"}}),
    json!({"env": {"A": "1", "B": "2", "C": "3"}}),
)]
#[case::top_level_null_overlay_skipped(
    json!({"a": 1}),
    json!(null),
    json!({"a": 1}),
)]
#[case::non_null_still_overwrites(
    json!({"a": 1}),
    json!({"a": 2, "b": 3}),
    json!({"a": 2, "b": 3}),
)]
fn deep_merge_non_null_cases(#[case] mut base: Value, #[case] overlay: Value, #[case] expected: Value) {
    deep_merge_non_null(&mut base, &overlay);
    assert_eq!(base, expected);
}
#[test]
fn deep_merge_non_null_concatenates_arrays() {
    let mut base = json!({"items": [1, 2]});
    let overlay = json!({"items": [3]});
    deep_merge_non_null(&mut base, &overlay);
    assert_eq!(base, json!({"items": [1, 2, 3]}));
}
