use std::ops::Range;
use std::sync::Arc;

use color_eyre::eyre::{Result, bail};
use nyne::router::AffectedFiles;

/// Parsed range from the `:{spec}` suffix. Set as request state by
/// [`SliceProvider`]. Downstream providers can read this to know slicing was
/// requested.
#[derive(Debug, Clone, Copy)]
pub struct SliceSpec {
    pub start: usize,
    pub end: Option<usize>,
}
impl SliceSpec {
    /// Convert the 1-based inclusive range to a 0-based `start..end` range,
    /// clamped to `line_count`. `:N` addresses line N only; `:N-M` addresses
    /// lines N through M inclusive.
    pub fn line_range(&self, line_count: usize) -> Range<usize> {
        let start = self.start.saturating_sub(1);
        let end = self.end.unwrap_or(self.start).min(line_count);
        start..end
    }
}
pub type SliceFn = Arc<dyn Fn(usize, usize) -> Result<Vec<u8>> + Send + Sync>;
/// Splice function: given (start, end, data) apply a range write.
/// Returns the source files affected by the splice.
pub type SpliceFn = Arc<dyn Fn(usize, usize, &[u8]) -> Result<AffectedFiles> + Send + Sync>;
#[derive(Clone)]
pub struct Sliceable {
    /// Custom read-slice: given (start, end) produce range-specific content.
    pub slice: SliceFn,
    /// Custom write-splice: given (start, end, data) apply a range write.
    pub splice: Option<SpliceFn>,
}
/// Parse `"name:M-N"` / `"name:M"` into `(base_name, spec)`.
/// Returns `None` if there's no `:` suffix or the suffix isn't a valid range.
pub fn parse_slice_suffix(name: &str) -> Option<(&str, &str)> {
    let colon = name.rfind(':')?;
    // Don't treat an empty suffix or leading colon as a slice
    if colon == 0 || colon == name.len() - 1 {
        return None;
    }
    let base = &name[..colon];
    let spec = &name[colon + 1..];
    // Validate the entire spec is a well-formed range (`\d+` or `\d+-\d+`).
    // A starts-with-digit check alone causes false positives on names like
    // `foo.rs:3--slug` (TODO entry filenames).
    if !is_valid_range_spec(spec) {
        return None;
    }
    Some((base, spec))
}
/// Check that `s` matches `\d+` or `\d+-\d+`.
fn is_valid_range_spec(s: &str) -> bool {
    match s.split_once('-') {
        Some((start, end)) =>
            !start.is_empty()
                && start.bytes().all(|b| b.is_ascii_digit())
                && !end.is_empty()
                && end.bytes().all(|b| b.is_ascii_digit()),
        None => !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()),
    }
}
/// Parse a range spec like `"10"`, `"10-20"` into a [`SliceSpec`].
pub fn parse_range(spec: &str) -> Result<SliceSpec> {
    if let Some((start_s, end_s)) = spec.split_once('-') {
        let start: usize = start_s
            .parse()
            .map_err(|_| color_eyre::eyre::eyre!("invalid range start: {start_s}"))?;
        let end: usize = end_s
            .parse()
            .map_err(|_| color_eyre::eyre::eyre!("invalid range end: {end_s}"))?;
        if end < start {
            bail!("range end ({end}) < start ({start})");
        }
        Ok(SliceSpec { start, end: Some(end) })
    } else {
        let start: usize = spec
            .parse()
            .map_err(|_| color_eyre::eyre::eyre!("invalid range: {spec}"))?;
        Ok(SliceSpec { start, end: None })
    }
}
