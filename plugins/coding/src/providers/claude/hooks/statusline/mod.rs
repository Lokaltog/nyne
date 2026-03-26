//! Statusline script — receives JSON payload on stdin, returns rendered ANSI on stdout.

/// Typed representation of the Claude Code statusline JSON payload.
mod payload;
/// ANSI statusline rendering segments.
mod render;
#[cfg(test)]
/// Statusline rendering tests.
mod tests;

use color_eyre::eyre::Result;
use nyne::dispatch::script::{Script, ScriptContext};

/// Statusline script implementation.
///
/// Receives a JSON payload on stdin describing the Claude Code session state
/// (model, context window usage, cost, rate limits, vim mode) and renders a
/// multi-line ANSI-colored status bar on stdout. The bar includes a gradient
/// progress indicator for context window consumption.
pub(in crate::providers::claude) struct Statusline;

/// [`Script`] implementation for [`Statusline`].
impl Script for Statusline {
    /// Parse the statusline JSON payload and render ANSI output.
    fn exec(&self, ctx: &ScriptContext<'_>, stdin: &[u8]) -> Result<Vec<u8>> {
        let payload: payload::StatuslinePayload = serde_json::from_slice(stdin)?;
        let render_ctx = render::Context {
            payload: &payload,
            activation: ctx.activation(),
        };
        Ok(render::render(&render_ctx).into_bytes())
    }
}
