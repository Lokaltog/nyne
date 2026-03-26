//! Shared JSON utilities.

use serde_json::Value;

/// Recursively merge `overlay` into `base`.
///
/// - Objects merge key-by-key (overlay keys win at leaf level).
/// - All other types (arrays, strings, numbers, bools, null) are replaced wholesale.
///
/// Based on the canonical recipe from the `serde_json` maintainer:
/// <https://github.com/serde-rs/json/issues/377>
pub fn deep_merge(base: &mut Value, overlay: &Value) { merge_inner(base, overlay, NullPolicy::Overwrite); }

/// Like [`deep_merge`], but null values in `overlay` are skipped rather than
/// overwriting the corresponding `base` entry.
///
/// Useful for merging config overlays where `null` means "not specified"
/// rather than "explicitly set to null".
pub fn deep_merge_non_null(base: &mut Value, overlay: &Value) { merge_inner(base, overlay, NullPolicy::Skip); }

/// Controls how `null` values in the overlay are handled during merge.
#[derive(Clone, Copy, PartialEq, Eq)]
enum NullPolicy {
    /// Null values overwrite the base (standard JSON merge).
    Overwrite,
    /// Null values are ignored, preserving the base entry.
    Skip,
}

/// Recursive merge implementation shared by [`deep_merge`] and [`deep_merge_non_null`].
fn merge_inner(base: &mut Value, overlay: &Value, null_policy: NullPolicy) {
    let skip = null_policy == NullPolicy::Skip;
    match (base, overlay) {
        // Null-skip path 1: object-into-object — skip individual null-valued keys.
        (Value::Object(base_map), Value::Object(overlay_map)) =>
            for (k, v) in overlay_map {
                if skip && v.is_null() {
                    continue;
                }
                merge_inner(base_map.entry(k.clone()).or_insert(Value::Null), v, null_policy);
            },
        (Value::Array(base_arr), Value::Array(overlay_arr)) => {
            base_arr.extend(overlay_arr.iter().cloned());
        }
        // Null-skip path 2: non-object base with null overlay — preserve the
        // existing base value. Without this guard the final arm would overwrite
        // base with null, defeating the Skip policy for scalar/array bases.
        (_, overlay) if skip && overlay.is_null() => {}
        (base, overlay) => *base = overlay.clone(),
    }
}

/// Unit tests.
#[cfg(test)]
mod tests;
