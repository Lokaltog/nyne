//! Generic deep-merge for layered configuration values.
//!
//! Provides a single [`deep_merge`] function that works with any type
//! implementing [`DeepMerge`] — currently [`serde_json::Value`] and
//! [`toml::Value`].

/// Trait abstracting the structural cases needed for recursive deep merge.
///
/// Implementors expose their map-like and array-like variants so the generic
/// [`deep_merge`] algorithm can operate without knowing the concrete value type.
pub trait DeepMerge: Clone {
    /// If both `self` and `overlay` are map-like, merge entries recursively
    /// using `recurse` and return `true`. Otherwise return `false`.
    fn merge_maps(&mut self, overlay: &Self, recurse: fn(&mut Self, &Self)) -> bool;

    /// If both `self` and `overlay` are array-like, extend `self` with
    /// `overlay`'s elements and return `true`. Otherwise return `false`.
    fn extend_arrays(&mut self, overlay: &Self) -> bool;
}

/// Recursively merge `overlay` into `base`.
///
/// - Maps merge key-by-key (overlay keys win at leaf level).
/// - Arrays extend (overlay appended to base).
/// - All other types are replaced wholesale.
pub fn deep_merge<T: DeepMerge>(base: &mut T, overlay: &T) {
    if base.merge_maps(overlay, deep_merge) {
        return;
    }
    if base.extend_arrays(overlay) {
        return;
    }
    *base = overlay.clone();
}
/// Generate a [`DeepMerge`] impl for a variant-structured value type.
///
/// Both `serde_json::Value` and `toml::Value` have a map-like variant,
/// an array-like variant, and leaf variants. The merge algorithm is
/// structurally identical in both cases; this macro factors out the
/// identical body so adding support for another value type would only
/// require naming its variants.
macro_rules! impl_deep_merge {
    ($ty:ty, $map_variant:ident, $array_variant:ident, $empty:expr) => {
        impl DeepMerge for $ty {
            fn merge_maps(&mut self, overlay: &Self, recurse: fn(&mut Self, &Self)) -> bool {
                let (Self::$map_variant(base), Self::$map_variant(over)) = (self, overlay) else {
                    return false;
                };
                for (k, v) in over {
                    recurse(base.entry(k.clone()).or_insert_with(|| $empty), v);
                }
                true
            }

            fn extend_arrays(&mut self, overlay: &Self) -> bool {
                let (Self::$array_variant(base), Self::$array_variant(over)) = (self, overlay) else {
                    return false;
                };
                base.extend(over.iter().cloned());
                true
            }
        }
    };
}

impl_deep_merge!(serde_json::Value, Object, Array, serde_json::Value::Null);
impl_deep_merge!(toml::Value, Table, Array, toml::Value::Table(toml::Table::new()));

#[cfg(test)]
mod tests;
