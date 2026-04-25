//! Post-tool-use Bash hints — suggests VFS alternatives per binary.
//!
//! Fires on `Bash` tool calls. Parses the command line, extracts the
//! base binary name and any root-relative paths, and renders a per-binary
//! hint (`cat` → use Read, `head`/`tail` → use symbol paths, etc.).

use std::sync::Arc;

use nyne::prelude::*;
use nyne::templates::TemplateEngine;
use nyne::{Script, ScriptContext};

use super::super::util;
use crate::provider::hook_schema::BashToolInput;

const TMPL: &str = "claude/post-tool-use-bash-hints";

/// `PostToolUse` Bash-hints script.
pub(in crate::provider) struct BashHints {
    pub(in crate::provider) engine: Arc<TemplateEngine>,
}

/// Build the template engine for the [`BashHints`] script.
pub(in crate::provider) fn build_engine() -> Arc<TemplateEngine> {
    let mut b = super::super::hook_builder();
    b.register(TMPL, include_str!("../templates/post-tool-use/bash-hints.md.j2"));
    b.finish()
}

/// [`Script`] implementation for [`BashHints`].
impl Script for BashHints {
    /// Extract `bin` and `rel_paths` from the bash command and render hints.
    fn exec(&self, ctx: &ScriptContext<'_>, stdin: &[u8]) -> Result<Vec<u8>> {
        Ok(util::run_script(
            ctx,
            stdin,
            &self.engine,
            TMPL,
            "PostToolUse",
            |input, ctx| {
                let cmd = input.tool_input_as::<BashToolInput>()?.command?;
                let rel_paths = util::extract_rel_paths(&cmd, ctx.activation().root(), ctx.chain());
                if rel_paths.is_empty() {
                    return None;
                }
                Some(minijinja::context! {
                    bin => util::extract_command_name(&cmd),
                    rel_paths,
                })
            },
        ))
    }
}
