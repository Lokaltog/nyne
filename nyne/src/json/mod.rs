//! Shared JSON utilities.

use serde_json::Value;

/// Recursively merge `overlay` into `base`.
///
/// - Objects merge key-by-key (overlay keys win at leaf level).
/// - All other types (arrays, strings, numbers, bools, null) are replaced wholesale.
///
/// Based on the canonical recipe from the `serde_json` maintainer:
/// <https://github.com/serde-rs/json/issues/377>
pub fn deep_merge(base: &mut Value, overlay: &Value) {
    match (base, overlay) {
        (Value::Object(base_map), Value::Object(overlay_map)) =>
            for (k, v) in overlay_map {
                deep_merge(base_map.entry(k.clone()).or_insert(Value::Null), v);
            },
        (base, overlay) => *base = overlay.clone(),
    }
}

/// Unit tests.
#[cfg(test)]
mod tests;
