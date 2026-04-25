//! Post-tool-use LSP diagnostics + static analysis hints.
//!
//! Fires on `Edit`/`Write` tool calls. Runs the `nyne_analysis` engine
//! (when the `analysis` feature is enabled) and fetches filtered LSP
//! diagnostics (when the `lsp` feature is enabled) for the changed
//! line range. Both feature flags are independent — either can be
//! disabled and the script still renders the other's output.

use std::sync::Arc;

use nyne::prelude::*;
use nyne::templates::TemplateEngine;
use nyne::{Script, ScriptContext};

use super::super::util;

const TMPL: &str = "claude/post-tool-use-diagnostics";

/// `PostToolUse` diagnostics + analysis script.
pub(in crate::provider) struct Diagnostics {
    pub(in crate::provider) engine: Arc<TemplateEngine>,
}

/// Build the template engine for the [`Diagnostics`] script.
pub(in crate::provider) fn build_engine() -> Arc<TemplateEngine> {
    let mut b = super::super::hook_builder();
    b.register(TMPL, include_str!("../templates/post-tool-use/diagnostics.md.j2"));
    b.finish()
}

/// [`Script`] implementation for [`Diagnostics`].
impl Script for Diagnostics {
    /// Run analysis + fetch LSP diagnostics and render both to the template.
    fn exec(&self, ctx: &ScriptContext<'_>, stdin: &[u8]) -> Result<Vec<u8>> {
        Ok(util::run_script(
            ctx,
            stdin,
            &self.engine,
            TMPL,
            "PostToolUse",
            |input, ctx| {
                let kind = input.tool_kind()?;
                let edit_input = input.edit_input();

                let rel =
                    util::source_rel_path(edit_input.as_ref(), input, kind, ctx.activation().root(), ctx.chain())?;

                #[cfg(feature = "lsp")]
                let (analysis, changed) = run_analysis(ctx, edit_input.as_ref(), &rel);
                #[cfg(not(feature = "lsp"))]
                let (analysis, _) = run_analysis(ctx, edit_input.as_ref(), &rel);

                #[cfg(feature = "lsp")]
                let diagnostics =
                    util::filter_diagnostics(util::fetch_diagnostics_for_tool(ctx, &rel), changed.as_ref());
                #[cfg(not(feature = "lsp"))]
                let diagnostics: Vec<minijinja::Value> = Vec::new();

                if analysis.is_empty() && diagnostics.is_empty() {
                    return None;
                }
                Some(minijinja::context! { rel, analysis, diagnostics })
            },
        ))
    }
}

#[cfg(not(feature = "analysis"))]
use stub::run_analysis;

#[cfg(feature = "analysis")]
use super::analysis::run_analysis;

/// No-op implementation used when the `analysis` feature is disabled.
#[cfg(not(feature = "analysis"))]
mod stub {
    use std::ops::Range;

    use nyne::ScriptContext;

    use crate::provider::hook_schema::EditToolInput;

    pub(super) fn run_analysis(
        _ctx: &ScriptContext<'_>,
        _edit_input: Option<&EditToolInput>,
        _rel: &str,
    ) -> (Vec<minijinja::Value>, Option<Range<usize>>) {
        (Vec::new(), None)
    }
}
