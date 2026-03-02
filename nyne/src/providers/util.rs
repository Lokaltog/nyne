//! Utilities for companion directory path manipulation.

const FALLBACK_EXT: &str = "ext";

/// Format detected languages as a comma-separated string (by frequency).
pub(super) fn languages_display(ext_counts: &[(String, usize)]) -> String {
    if ext_counts.is_empty() {
        "none detected".into()
    } else {
        ext_counts
            .iter()
            .map(|(ext, _)| ext.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Dominant non-markdown extension for code examples.
pub(super) fn dominant_ext(ext_counts: &[(String, usize)]) -> &str {
    ext_counts
        .iter()
        .find(|(ext, _)| ext != "md")
        .map_or(FALLBACK_EXT, |(ext, _)| ext.as_str())
}
