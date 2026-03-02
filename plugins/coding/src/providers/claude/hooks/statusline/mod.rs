//! Statusline script — receives JSON payload on stdin, returns rendered ANSI on stdout.

mod payload;
mod render;
#[cfg(test)]
mod tests;

use color_eyre::eyre::Result;
use nyne::dispatch::script::{Script, ScriptContext};

/// Statusline script implementation.
pub(in crate::providers::claude) struct Statusline;

impl Script for Statusline {
    fn exec(&self, ctx: &ScriptContext<'_>, stdin: &[u8]) -> Result<Vec<u8>> {
        let payload: payload::StatuslinePayload = serde_json::from_slice(stdin)?;
        let render_ctx = render::Context {
            payload: &payload,
            activation: ctx.activation(),
        };
        Ok(render::render(&render_ctx).into_bytes())
    }
}
