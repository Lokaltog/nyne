//! Post-tool-use SSOT/DRY reminder on significant edits.
//!
//! Fires on `Edit`/`Write` tool calls. For `Write` any invocation
//! triggers the reminder. For `Edit`, the combined line count of
//! `old_string` + `new_string` must exceed [`SSOT_LINE_THRESHOLD`] —
//! small tweaks (renames, one-liners) are not worth the noise.

use std::sync::Arc;

use nyne::prelude::*;
use nyne::templates::TemplateEngine;
use nyne::{Script, ScriptContext};

use super::super::util;
use crate::provider::hook_schema::{EditToolInput, ToolKind};

const TMPL: &str = "claude/post-tool-use-ssot";

/// Minimum combined old+new line count to trigger SSOT reminder on Edit.
///
/// When an Edit tool call's `old_string` + `new_string` combined line count
/// exceeds this threshold, the post-hook emits an SSOT/DRY check reminder.
/// Small edits (renames, one-liners) are not worth the noise.
const SSOT_LINE_THRESHOLD: usize = 10;

/// `PostToolUse` SSOT/DRY reminder script.
pub(in crate::provider) struct Ssot {
    pub(in crate::provider) engine: Arc<TemplateEngine>,
}

/// Build the template engine for the [`Ssot`] script.
pub(in crate::provider) fn build_engine() -> Arc<TemplateEngine> {
    let mut b = super::super::hook_builder();
    b.register(TMPL, include_str!("../templates/post-tool-use/ssot.md.j2"));
    b.finish()
}

/// [`Script`] implementation for [`Ssot`].
impl Script for Ssot {
    /// Emit an SSOT/DRY reminder for significant edits or any write.
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

                if !should_trigger(kind, edit_input.as_ref()) {
                    return None;
                }

                let rel =
                    util::source_rel_path(edit_input.as_ref(), input, kind, ctx.activation().root(), ctx.chain())?;
                Some(minijinja::context! { rel })
            },
        ))
    }
}

/// Whether this tool call meets the threshold for an SSOT reminder.
///
/// `Write` always triggers; `Edit` triggers only when the combined
/// line count of `old_string` + `new_string` exceeds
/// [`SSOT_LINE_THRESHOLD`]. Read and non-file tools never trigger.
fn should_trigger(kind: ToolKind, edit: Option<&EditToolInput>) -> bool {
    match kind {
        ToolKind::Write => true,
        ToolKind::Edit => edit.is_some_and(|e| {
            let old = e.old_string.as_deref().map_or(0, |s| s.lines().count());
            let new = e.new_string.as_deref().map_or(0, |s| s.lines().count());
            old + new > SSOT_LINE_THRESHOLD
        }),
        ToolKind::Read => false,
    }
}
