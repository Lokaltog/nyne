//! Provider utilities for nyne-coding.
//!
//! Small helpers shared across multiple providers that don't belong to
//! a specific provider module.

use nyne::dispatch::activation::ActivationContext;
use nyne::types::ExtensionCounts;

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
