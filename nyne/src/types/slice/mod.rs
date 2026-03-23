//! Slice specification parsing for list-like virtual files.
//!
//! Re-used by `node::line_slice` (core) and plugin providers.

use std::ops::Range;

/// A parsed range specification from a `:M`, `:M-N`, or `:-N` suffix.
///
/// This is the generic slicing pattern described in the VFS spec — available
/// on any list-like virtual file. Sliced paths are always lookup-only (hidden
/// from readdir).
///
/// All indices are 1-based and inclusive (matching `sed`/`awk` conventions).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliceSpec {
    /// Single item at 1-based index.
    Single(usize),
    /// Inclusive range `[start, end]`, both 1-based.
    Range(usize, usize),
    /// Last N items from the end (e.g., `:-10`).
    Tail(usize),
}

/// Parse a colon-suffixed slice spec from a name.
///
/// Returns `Some((base_name, spec))` if the name ends with a valid `:…`
/// suffix. Returns `None` if there is no colon, the base is empty, or the
/// suffix doesn't parse.
pub fn parse_slice_suffix(name: &str) -> Option<(&str, SliceSpec)> {
    let (base, spec_str) = name.rsplit_once(':')?;
    if base.is_empty() {
        return None;
    }
    Some((base, parse_spec(spec_str)?))
}

/// Parse a bare spec string into a [`SliceSpec`].
///
/// Accepts `"5"` (single), `"5-10"` (range), or `"-10"` (tail).
/// Used by [`parse_slice_suffix`] and by route handlers that receive
/// the spec as a captured route parameter.
pub fn parse_spec(s: &str) -> Option<SliceSpec> {
    // Tail: "-N" where N > 0
    if let Some(rest) = s.strip_prefix('-') {
        if rest.is_empty() {
            return None;
        }
        let n: usize = rest.parse().ok()?;
        return (n > 0).then_some(SliceSpec::Tail(n));
    }

    // Range: "M-N"
    if let Some((start_str, end_str)) = s.split_once('-') {
        let start: usize = start_str.parse().ok()?;
        let end: usize = end_str.parse().ok()?;
        if start == 0 || end < start {
            return None;
        }
        return Some(SliceSpec::Range(start, end));
    }

    // Single: "M"
    let m: usize = s.parse().ok()?;
    (m > 0).then_some(SliceSpec::Single(m))
}

impl SliceSpec {
    /// Compute the 0-based half-open index range for a collection of `total` items.
    ///
    /// Out-of-range indices are clamped silently (matching `sed` behaviour).
    #[must_use]
    pub fn index_range(&self, total: usize) -> Range<usize> {
        match *self {
            Self::Single(m) => {
                let idx = m.saturating_sub(1).min(total);
                idx..idx.saturating_add(1).min(total)
            }
            Self::Range(start, end) => start.saturating_sub(1).min(total)..end.min(total),
            Self::Tail(n) => total.saturating_sub(n)..total,
        }
    }

    /// Apply this slice to a list of items, returning the selected sub-slice.
    #[must_use]
    pub fn apply<'a, T>(&self, items: &'a [T]) -> &'a [T] { items.get(self.index_range(items.len())).unwrap_or(&[]) }
}

#[cfg(test)]
mod tests;
