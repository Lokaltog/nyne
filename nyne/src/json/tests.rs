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
