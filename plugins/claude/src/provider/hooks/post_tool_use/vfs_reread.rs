//! Post-tool-use re-read reminder after VFS writes.
//!
//! Fires on `Edit`/`Write` tool calls. If the write targeted a VFS
//! companion path, surfaces a reminder to re-read `OVERVIEW.md` for
//! the underlying file — symbol names may have shifted due to the
//! re-parse that every write triggers.

use std::path::Path;
use std::sync::Arc;

use nyne::prelude::*;
use nyne::templates::TemplateEngine;
use nyne::{Script, ScriptContext};

use super::super::util;
use crate::provider::hook_schema::EditToolInput;

const TMPL: &str = "claude/post-tool-use-vfs-reread";

/// `PostToolUse` VFS re-read reminder script.
pub(in crate::provider) struct VfsReread {
    pub(in crate::provider) engine: Arc<TemplateEngine>,
}

/// Build the template engine for the [`VfsReread`] script.
pub(in crate::provider) fn build_engine() -> Arc<TemplateEngine> {
    let mut b = super::super::hook_builder();
    b.register(TMPL, include_str!("../templates/post-tool-use/vfs-reread.md.j2"));
    b.finish()
}

/// [`Script`] implementation for [`VfsReread`].
impl Script for VfsReread {
    /// Emit a re-read reminder when Edit/Write targets a VFS path.
    fn exec(&self, ctx: &ScriptContext<'_>, stdin: &[u8]) -> Result<Vec<u8>> {
        Ok(util::run_script(
            ctx,
            stdin,
            &self.engine,
            TMPL,
            "PostToolUse",
            |input, ctx| {
                let kind = input.tool_kind()?;
                let edit_input = input.tool_input_as::<EditToolInput>();
                let file_path = util::tool_file_path(edit_input.as_ref(), input, kind)?;
                let root = ctx.activation().root();
                let companion = super::super::resolve_companion(ctx.chain(), root, Path::new(&file_path))?;
                let rel = companion.source_file?.to_str()?.to_owned();
                Some(minijinja::context! { rel, is_vfs => true })
            },
        ))
    }
}
