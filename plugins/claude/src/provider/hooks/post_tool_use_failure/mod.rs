//! `PostToolUseFailure` hook — fires when a tool call fails.
//!
//! Detects the specific case where a VFS symbol body edit succeeds on disk
//! but returns ENOENT because the edit renamed the symbol (the old path no
//! longer exists). Emits a context message explaining the edit succeeded.

use std::sync::Arc;

use color_eyre::eyre::Result;
use nyne::templates::TemplateEngine;
use nyne::{Script, ScriptContext};

use crate::provider::hook_schema::{EditToolInput, HookInput, HookOutput};

/// Template key for the symbol-renamed recovery hint.
const TMPL: &str = "claude/post-tool-use-symbol-renamed";

/// `PostToolUseFailure` hook script implementation.
pub(in crate::provider) struct PostToolUseFailure {
    pub(in crate::provider) engine: Arc<TemplateEngine>,
}

pub(in crate::provider) fn build_engine() -> Arc<TemplateEngine> {
    let mut b = super::hook_builder();
    b.register(TMPL, include_str!("../templates/post-tool-use/symbol-renamed.md.j2"));
    b.finish()
}

impl Script for PostToolUseFailure {
    fn exec(&self, ctx: &ScriptContext<'_>, stdin: &[u8]) -> Result<Vec<u8>> {
        let Some(input) = HookInput::parse(stdin) else {
            return Ok(HookOutput::empty());
        };

        let is_enoent = input.error.as_deref().is_some_and(|e| e.contains("ENOENT"));

        if !is_enoent {
            return Ok(HookOutput::empty());
        }

        let edit_input = input.tool_input_as::<EditToolInput>();
        let file_path = edit_input.as_ref().and_then(|e| e.file_path.as_deref());

        let is_vfs_symbol_edit = file_path.is_some_and(|fp| fp.contains("@/symbols/"));

        if !is_vfs_symbol_edit {
            return Ok(HookOutput::empty());
        }

        // Resolve the source file's relative path for the context message.
        let root = ctx.activation().root_prefix();
        let rel = file_path.and_then(|fp| {
            let chain = ctx.chain();
            match super::resolve_companion(chain, root, fp) {
                Some(c) => c.source_file.and_then(|sf| sf.to_str().map(str::to_owned)),
                None => fp.strip_prefix(root).map(str::to_owned),
            }
        });

        let view = minijinja::context! { is_vfs_symbol_edit, is_enoent, rel };
        Ok(super::render_context(&self.engine, TMPL, &view, "PostToolUseFailure"))
    }
}
