use std::ops::Range;

/// 0-based line number for a byte offset in source text.
///
/// Counts newlines preceding the offset. Single source of truth for
/// byte-offset → line-number conversion.
pub fn line_of_byte(source: &str, byte: usize) -> usize { source[..byte].bytes().filter(|&b| b == b'\n').count() }

/// Line range metadata attached to symbol directory nodes.
///
/// Providers that create symbol decompositions attach this to fragment
/// directory `VirtualNode`s via [`VirtualNode::prop`](crate::node::VirtualNode::prop). Other providers
/// (e.g., git) read it via the resolver to scope operations to a
/// symbol's line range.
///
/// Line numbers are 1-based, matching the VFS convention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolLineRange {
    /// 1-based inclusive start line.
    pub start: usize,
    /// 1-based inclusive end line.
    pub end: usize,
}

impl SymbolLineRange {
    /// Create from a 0-based exclusive `Range<usize>` (tree-sitter convention).
    pub const fn from_zero_based(range: &Range<usize>) -> Self {
        debug_assert!(range.start < range.end, "SymbolLineRange: empty or inverted range");
        Self {
            start: range.start + 1,
            end: range.end,
        }
    }

    /// Create from a byte range in the source text.
    pub fn from_byte_range(source: &str, byte_range: &Range<usize>) -> Self {
        let start_line = line_of_byte(source, byte_range.start);
        let end_line = line_of_byte(source, byte_range.end) + 1;
        Self::from_zero_based(&(start_line..end_line))
    }

    /// Format as a `lines:M-N` suffix string (or `lines:M` for single-line ranges).
    pub fn as_lines_suffix(&self) -> String {
        if self.start == self.end {
            format!("lines:{}", self.start)
        } else {
            format!("lines:{}-{}", self.start, self.end)
        }
    }
}
