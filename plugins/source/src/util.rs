//! Provider utilities for nyne-source.
//!
//! Small helpers shared across multiple providers that don't belong to
//! a specific provider module.

use nyne::{ActivationContext, ExtensionCounts};

/// Fallback file extension used when no dominant extension is found.
///
/// Returned by [`dominant_ext`] when the activation context has no extension
/// counts or every counted extension is `"md"`. The generic `"ext"` value
/// avoids empty-string edge cases in callers that need *some* extension.
const FALLBACK_EXT: &str = "ext";

/// Get the dominant non-markdown extension from the activation context.
///
/// Returns the most-common file extension in the mounted directory, excluding
/// `"md"` (markdown), which is typically ancillary documentation rather than
/// primary source code. Falls back to [`FALLBACK_EXT`] when no qualifying
/// extension exists.
pub fn dominant_ext(ctx: &ActivationContext) -> String {
    ctx.get::<ExtensionCounts>()
        .and_then(|c| {
            c.as_slice()
                .iter()
                .find(|(ext, _)| ext != "md")
                .map(|(ext, _)| ext.clone())
        })
        .unwrap_or_else(|| FALLBACK_EXT.to_owned())
}

/// Given text immediately after a tag keyword (e.g. the `": fix this"` in
/// `TODO: fix this`), skip an optional `(annotation)` and require a colon.
///
/// Returns `None` if no colon follows the tag — bare mentions are not actionable.
///
/// # Examples
///
/// - `": fix this"` → `Some("fix this")`
/// - `"(user): fix"` → `Some("fix")`
/// - `" bare mention"` → `None`
pub fn parse_tag_suffix(after_tag: &str) -> Option<&str> {
    // Skip optional parenthesized annotation like `(scope)`.
    let rest = if after_tag.starts_with('(') {
        after_tag.find(')').map_or(after_tag, |pos| &after_tag[pos + 1..])
    } else {
        after_tag
    };
    // Require a colon — bare mentions are not actionable.
    Some(rest.strip_prefix(':')?.trim())
}
