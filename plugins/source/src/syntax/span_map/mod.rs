//! Byte-range remapping for injection-based compound decomposition.
//!
//! When a compound file (e.g. `.md.j2`) is decomposed, the inner language's
//! decomposer operates on concatenated content regions — a "virtual" byte
//! space. [`SpanMap`] translates virtual byte offsets back to their real
//! positions in the original file.

use std::ops::Range;

use super::fragment::Fragment;

/// Maps virtual byte offsets (in concatenated content regions) to real byte
/// offsets (in the original compound file).
///
/// Built from a list of content regions `(real_start, length)`. The virtual
/// address space is the regions laid end-to-end starting at 0.
#[derive(Debug, Clone)]
pub struct SpanMap {
    regions: Vec<Region>,
    virtual_len: usize,
}

/// A single content region mapping virtual to real byte offsets.
///
/// Each region represents a contiguous slice of inner-language content
/// in the original file. `virtual_start` is the offset in the concatenated
/// content string; `real_start` is the offset in the original compound file.
/// `len` is the same in both address spaces (content is copied verbatim).
#[derive(Debug, Clone)]
struct Region {
    real_start: usize,
    virtual_start: usize,
    len: usize,
}

/// Construction, translation, and remapping methods for `SpanMap`.
impl SpanMap {
    /// Build a span map and concatenated inner content from the original source
    /// and content region byte ranges.
    ///
    /// Returns both the map and the concatenated content string. This is the
    /// primary constructor — it enforces that the span map and the content
    /// the inner decomposer operates on are derived from the same regions.
    ///
    /// Zero-length regions are silently skipped.
    pub(crate) fn build(source: &str, regions: &[Range<usize>]) -> (Self, String) {
        // Clamp regions to source bounds FIRST, then build both the map
        // and content from the clamped list. This is the critical SSOT
        // invariant: map.virtual_len() == content.len() always.
        let clamped: Vec<(usize, usize)> = regions
            .iter()
            .map(|r| {
                let end = r.end.min(source.len());
                let start = r.start.min(end);
                (start, end - start)
            })
            .collect();

        let map = Self::new(&clamped);
        let mut content = String::with_capacity(map.virtual_len);
        for &(start, len) in &clamped {
            if len == 0 {
                continue;
            }
            content.push_str(source.get(start..start + len).unwrap_or_default());
        }
        debug_assert_eq!(content.len(), map.virtual_len, "SpanMap/content length divergence");
        (map, content)
    }

    /// Build a span map from `(real_start, length)` pairs.
    ///
    /// Prefer [`build`](Self::build) when you also need the concatenated
    /// content — it guarantees the map and content stay in sync.
    ///
    /// Zero-length regions are silently skipped.
    pub(crate) fn new(regions: &[(usize, usize)]) -> Self {
        let mut mapped = Vec::with_capacity(regions.len());
        let mut virtual_cursor = 0;

        for &(real_start, len) in regions {
            if len == 0 {
                continue;
            }
            mapped.push(Region {
                real_start,
                virtual_start: virtual_cursor,
                len,
            });
            virtual_cursor += len;
        }

        Self {
            regions: mapped,
            virtual_len: virtual_cursor,
        }
    }

    /// Total length of the virtual (concatenated) content.
    #[cfg(test)]
    pub(crate) const fn virtual_len(&self) -> usize { self.virtual_len }

    /// Index of the region containing `virtual_offset`.
    ///
    /// Returns the last region whose `virtual_start <= virtual_offset`.
    /// `None` only when the map is empty.
    fn region_index(&self, virtual_offset: usize) -> Option<usize> {
        self.regions
            .partition_point(|r| r.virtual_start <= virtual_offset)
            .checked_sub(1)
    }

    /// Translate a single virtual byte offset to its real position.
    ///
    /// For offsets before any region (only possible with an empty map),
    /// the offset is returned unchanged.
    pub(crate) fn to_real(&self, virtual_offset: usize) -> usize {
        match self.region_index(virtual_offset).and_then(|i| self.regions.get(i)) {
            Some(region) => {
                let offset_within = virtual_offset - region.virtual_start;
                region.real_start + offset_within
            }
            None => virtual_offset,
        }
    }

    /// Remap a virtual byte range to real coordinates.
    ///
    /// When the range's start and end fall in the same content region, the
    /// mapping is straightforward. When they span across different regions
    /// (i.e. across a template directive gap), the end is clamped to the
    /// real end of the region containing the start — inner-language
    /// fragments must never bleed into template directives.
    ///
    /// The exclusive end is mapped as `to_real(end - 1) + 1` so that a
    /// range ending exactly at a region boundary stays within the preceding
    /// region rather than jumping to the next one's start.
    pub(crate) fn remap_range(&self, range: Range<usize>) -> Range<usize> {
        if range.end <= range.start {
            let start = self.to_real(range.start);
            return start..start;
        }

        let start_idx = self.region_index(range.start);
        let end_idx = self.region_index(range.end - 1);
        let start = self.to_real(range.start);

        let end = match (start_idx, end_idx) {
            (Some(si), Some(ei)) if si == ei => {
                // Same region — map directly.
                self.to_real(range.end - 1) + 1
            }
            (Some(si), Some(_)) => {
                // Cross-region — clamp to the start region's real end.
                // `si` is always valid (from `region_index`).
                self.regions.get(si).map_or_else(
                    || self.to_real(range.end - 1) + 1,
                    |region| region.real_start + region.len,
                )
            }
            _ => self.to_real(range.end - 1) + 1,
        };

        start..end
    }

    /// Deep-remap all byte-offset fields in a fragment and its children.
    ///
    /// Remaps `byte_range` and `name_byte_offset` on the fragment's
    /// [`FragmentSpan`], recurses into children, then recomputes the
    /// cached `full_span` so it reflects the real (post-remap) child
    /// positions.
    pub(crate) fn remap_fragment(&self, mut fragment: Fragment) -> Fragment {
        fragment.span.byte_range = self.remap_range(fragment.span.byte_range);
        fragment.span.name_byte_offset = self.to_real(fragment.span.name_byte_offset);

        fragment.children = fragment.children.into_iter().map(|c| self.remap_fragment(c)).collect();
        fragment.span.recompute_full_span(&fragment.children);

        fragment
    }
}

/// Tests for span map remapping logic.
#[cfg(test)]
mod tests;
