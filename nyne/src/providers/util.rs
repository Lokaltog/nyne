//! Shared utilities for core providers.
//!
//! Helpers for formatting language distribution and selecting dominant file
//! extensions from [`ExtensionCounts`](crate::types::ExtensionCounts). Used
//! by the directory and nyne providers to populate template views.

/// Fallback file extension when no dominant language is detected.
const FALLBACK_EXT: &str = "ext";

/// Format detected languages as a comma-separated string, ordered by frequency.
///
/// Returns `"none detected"` when the extension counts are empty.
/// Used in GUIDE.md and OVERVIEW.md templates to display the project's
/// language distribution.
pub(super) fn languages_display(ext_counts: &[(String, usize)]) -> String {
    if ext_counts.is_empty() {
        "none detected".into()
    } else {
        ext_counts
            .iter()
            .enumerate()
            .fold(String::new(), |mut acc, (i, (ext, _))| {
                if i > 0 {
                    acc.push_str(", ");
                }
                acc.push_str(ext);
                acc
            })
    }
}

/// Return the most frequent non-markdown file extension for code examples.
///
/// Skips `"md"` because markdown is documentation, not the project's primary
/// language. Falls back to [`FALLBACK_EXT`] when no non-markdown extension
/// exists. Used by templates to choose the fence language tag for code blocks.
pub(super) fn dominant_ext(ext_counts: &[(String, usize)]) -> &str {
    ext_counts
        .iter()
        .find(|(ext, _)| ext != "md")
        .map_or(FALLBACK_EXT, |(ext, _)| ext.as_str())
}
