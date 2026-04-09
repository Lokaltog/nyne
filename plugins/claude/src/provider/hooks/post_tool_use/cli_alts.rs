//! Post-tool-use CLI alternatives — suggests `rg`/`ast-grep`/`fd` over
//! the built-in `Grep`/`Glob` tools, plus VFS navigation paths.
//!
//! Fires on `Grep` and `Glob` tool calls. Pulls the raw pattern from
//! `tool_input` and renders a tool-specific alternative hint.

use std::sync::Arc;

use nyne::prelude::*;
use nyne::templates::TemplateEngine;
use nyne::{Script, ScriptContext};

use super::super::util;
use crate::provider::hook_schema::{GlobToolInput, GrepToolInput};

const TMPL: &str = "claude/post-tool-use-cli-alts";

/// `PostToolUse` CLI alternatives script.
pub(in crate::provider) struct CliAlts {
    pub(in crate::provider) engine: Arc<TemplateEngine>,
}

/// Build the template engine for the [`CliAlts`] script.
pub(in crate::provider) fn build_engine() -> Arc<TemplateEngine> {
    let mut b = super::super::hook_builder();
    b.register(TMPL, include_str!("../templates/post-tool-use/cli-alts.md.j2"));
    b.finish()
}

/// [`Script`] implementation for [`CliAlts`].
impl Script for CliAlts {
    /// Extract the search pattern and render CLI-alternative hints.
    fn exec(&self, ctx: &ScriptContext<'_>, stdin: &[u8]) -> Result<Vec<u8>> {
        Ok(util::run_script(
            ctx,
            stdin,
            &self.engine,
            TMPL,
            "PostToolUse",
            |input, _ctx| {
                let tool_name = input.tool_name.as_deref()?;
                let pattern = match tool_name {
                    "Grep" => input.tool_input_as::<GrepToolInput>()?.pattern?,
                    "Glob" => input.tool_input_as::<GlobToolInput>()?.pattern?,
                    _ => return None,
                };
                Some(minijinja::context! { tool_name, pattern })
            },
        ))
    }
}
