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

impl DeepMerge for serde_json::Value {
    fn merge_maps(&mut self, overlay: &Self, recurse: fn(&mut Self, &Self)) -> bool {
        let (Self::Object(base_map), Self::Object(overlay_map)) = (self, overlay) else {
            return false;
        };
        for (k, v) in overlay_map {
            recurse(base_map.entry(k.clone()).or_insert(Self::Null), v);
        }
        true
    }

    fn extend_arrays(&mut self, overlay: &Self) -> bool {
        let (Self::Array(base_arr), Self::Array(overlay_arr)) = (self, overlay) else {
            return false;
        };
        base_arr.extend(overlay_arr.iter().cloned());
        true
    }
}

impl DeepMerge for toml::Value {
    fn merge_maps(&mut self, overlay: &Self, recurse: fn(&mut Self, &Self)) -> bool {
        let (Self::Table(base_table), Self::Table(overlay_table)) = (self, overlay) else {
            return false;
        };
        for (k, v) in overlay_table {
            recurse(base_table.entry(k).or_insert(Self::Table(toml::Table::new())), v);
        }
        true
    }

    fn extend_arrays(&mut self, overlay: &Self) -> bool {
        let (Self::Array(base_arr), Self::Array(overlay_arr)) = (self, overlay) else {
            return false;
        };
        base_arr.extend(overlay_arr.iter().cloned());
        true
    }
}

#[cfg(test)]
mod tests;
