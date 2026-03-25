//! Provider utilities for nyne-coding.

use nyne::dispatch::activation::ActivationContext;
use nyne::types::ExtensionCounts;

/// Fallback file extension used when no dominant extension is found.
const FALLBACK_EXT: &str = "ext";

/// Get the dominant non-markdown extension from the activation context.
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
