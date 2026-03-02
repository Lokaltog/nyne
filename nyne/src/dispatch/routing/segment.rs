/// Segment pattern types for route matching.
///
/// Domain-agnostic — no VFS-specific variants. Generic parametric
/// matching like a web router (axum, Hono).
#[derive(Debug, Clone)]
pub enum SegmentMatcher {
    /// Exact literal: `"git"`, `"@"`, `"branches"`
    Exact(&'static str),
    /// Named capture: `"{source}"` — captures one segment.
    /// Optional prefix: `"BLAME.md:{spec}"` strips `BLAME.md:` before capturing.
    /// Optional suffix: `"{source}@"` strips `@` from the captured value.
    Capture {
        name: &'static str,
        prefix: Option<&'static str>,
        suffix: Option<&'static str>,
    },
    /// Rest capture: `"{..path}"` — captures 1+ segments.
    /// Optional suffix: `"{..path}@"` — rightmost-suffix matching [DD-19].
    RestCapture {
        name: &'static str,
        suffix: Option<&'static str>,
    },
    /// Glob: `"**"` — matches any depth.
    Glob,
    /// Root node (implicit, matches mount root).
    Root,
}

/// Result of a single-segment match attempt.
#[derive(Debug)]
pub(super) enum CaptureResult {
    /// No capture (exact or glob match).
    None,
    /// Single-segment capture: (name, `captured_value`).
    Single(&'static str, String),
}

impl SegmentMatcher {
    /// Match a single path segment against this pattern.
    ///
    /// Returns `None` if no match. `RestCapture` and `Root` are handled
    /// by the tree walk, not by this method.
    pub(super) fn matches(&self, segment: &str) -> Option<CaptureResult> {
        match self {
            Self::Exact(expected) => (segment == *expected).then_some(CaptureResult::None),
            Self::Capture { name, prefix, suffix } => {
                let remaining = if let Some(pfx) = prefix {
                    segment.strip_prefix(pfx)?
                } else {
                    segment
                };
                let captured = if let Some(sfx) = suffix {
                    remaining.strip_suffix(sfx)?
                } else {
                    remaining
                };
                (!captured.is_empty()).then(|| CaptureResult::Single(name, captured.to_owned()))
            }
            // Glob, Root, RestCapture are handled by the tree walk, not by matches()
            Self::Glob | Self::Root | Self::RestCapture { .. } => Option::None,
        }
    }

    /// Precedence rank for deterministic matching order [DD-21].
    /// Lower is higher priority.
    pub(super) const fn precedence(&self) -> u8 {
        match self {
            Self::Root => 0,
            Self::Exact(_) => 1,
            // Captures with prefix/suffix are more specific than bare captures.
            Self::Capture { prefix: Some(_), .. } | Self::Capture { suffix: Some(_), .. } => 2,
            Self::Capture { .. } => 3,
            Self::RestCapture { .. } => 4,
            Self::Glob => 5,
        }
    }
}
