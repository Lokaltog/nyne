//! TOML deep merge for layered configuration.
//!
//! Mirrors the semantics of [`crate::json::deep_merge`] but operates on
//! [`toml::Value`] directly, avoiding any format crossing.

/// Recursively merge `overlay` into `base`.
///
/// - Tables merge key-by-key (overlay keys win at leaf level).
/// - All other types (arrays, strings, integers, bools, datetimes) are replaced wholesale.
pub fn deep_merge(base: &mut toml::Value, overlay: &toml::Value) {
    match (base, overlay) {
        (toml::Value::Table(base_table), toml::Value::Table(overlay_table)) =>
            for (k, v) in overlay_table {
                deep_merge(base_table.entry(k).or_insert(toml::Value::Table(toml::Table::new())), v);
            },
        (base, overlay) => *base = overlay.clone(),
    }
}

#[cfg(test)]
mod tests;
