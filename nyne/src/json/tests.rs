use rstest::rstest;
use serde_json::{Value, json};

use super::deep_merge;

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
#[case::arrays_replaced(
    json!({"items": [1, 2]}),
    json!({"items": [3]}),
    json!({"items": [3]}),
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
fn deep_merge_cases(#[case] mut base: Value, #[case] overlay: Value, #[case] expected: Value) {
    deep_merge(&mut base, &overlay);
    assert_eq!(base, expected);
}
