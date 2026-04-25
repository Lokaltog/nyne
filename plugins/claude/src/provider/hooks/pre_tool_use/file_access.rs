//! Pre-tool-use file-access guard — deny vs hint for raw source files.
//!
//! Fires on `Read`/`Edit`/`Write` tool calls targeting paths under the
//! mount root. For files with symbol decomposition, either emits a hint
//! (suggesting the `@/` namespace) or denies the raw access outright for
//! broad reads that exceed the configured deny-lines threshold.

use std::path::Path;
use std::sync::Arc;

use nyne::prelude::*;
use nyne::templates::TemplateEngine;
use nyne::{Script, ScriptContext};
use nyne_source::{
    DecomposedSource, SYMBOL_TABLE_PARTIAL_KEY, SYMBOL_TABLE_PARTIAL_SRC, SourceContextExt as _, fragment_list,
};
use serde::Serialize;

use super::super::util;
use crate::plugin::config::{PreToolHookConfig, PreToolPolicy};
use crate::provider::hook_schema::{EditToolInput, HookInput, HookOutput, ReadToolInput, ToolKind, WriteToolInput};

const TMPL: &str = "claude/pre-tool-use-file-access";
const PARTIAL_DENY: &str = "hooks/pre-tool-use/deny";
const PARTIAL_HINT: &str = "hooks/pre-tool-use/hint";

/// Mode for the file-access template dispatch.
///
/// Determines both the Jinja partial to render and the output type
/// (deny vs context hint). Serializes to lowercase for template matching.
#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Mode {
    /// Suggest VFS alternatives without blocking the tool call.
    Hint,
    /// Block the tool call (e.g., broad raw-file reads).
    Deny,
}

/// `PreToolUse` file-access guard script.
pub(in crate::provider) struct FileAccess {
    pub(in crate::provider) engine: Arc<TemplateEngine>,
    pub(in crate::provider) config: PreToolHookConfig,
}

/// Build the template engine for the [`FileAccess`] script.
pub(in crate::provider) fn build_engine() -> Arc<TemplateEngine> {
    let mut b = super::super::hook_builder();
    b.register_partial(SYMBOL_TABLE_PARTIAL_KEY, SYMBOL_TABLE_PARTIAL_SRC);
    b.register_partial(PARTIAL_DENY, include_str!("../templates/pre-tool-use/deny.md.j2"));
    b.register_partial(PARTIAL_HINT, include_str!("../templates/pre-tool-use/hint.md.j2"));
    b.register(TMPL, include_str!("../templates/pre-tool-use/file-access.md.j2"));
    b.finish()
}

/// Parsed and validated target of a file-access tool call.
///
/// Produced by [`resolve_target`] once all pass-through checks (path
/// under root, not a companion path, has decomposition, grace period
/// expired) have succeeded. Carries the resolved [`ToolKind`] and a
/// one-shot-computed `sym` so the rendering logic never re-parses the
/// tool name or re-resolves the symbol.
struct Target<'a> {
    kind: ToolKind,
    rel: &'a str,
    read_input: Option<ReadToolInput>,
    decomposed: Arc<DecomposedSource>,
    /// Enclosing symbol name for the targeted offset or `old_string`.
    /// Resolved once during [`resolve_target`].
    sym: Option<String>,
}

/// [`Script`] implementation for [`FileAccess`].
impl Script for FileAccess {
    /// Parse the tool input, resolve the target, and render the response.
    fn exec(&self, ctx: &ScriptContext<'_>, stdin: &[u8]) -> Result<Vec<u8>> {
        let Some(input) = HookInput::parse(stdin) else {
            return Ok(HookOutput::empty());
        };
        let Some(kind) = input.tool_kind() else {
            return Ok(HookOutput::empty());
        };
        let Some((file_path, read_input, edit_input)) = take_target_path(kind, &input) else {
            return Ok(HookOutput::empty());
        };
        let Some(target) = resolve_target(ctx, &file_path, kind, read_input, edit_input.as_ref()) else {
            return Ok(HookOutput::empty());
        };
        Ok(self.render_response(&target))
    }
}

/// Rendering logic for [`FileAccess`] — split from the trait impl for clarity.
impl FileAccess {
    /// Render the deny-or-hint response for a resolved target.
    fn render_response(&self, target: &Target<'_>) -> Vec<u8> {
        let policy = self.config.resolve(target.decomposed.decomposer.language_name());
        let mode = pick_mode(target, &policy);
        let trimmed = self
            .engine
            .render(TMPL, &build_view(target, mode, &policy))
            .trim()
            .to_owned();

        if trimmed.is_empty() {
            return HookOutput::empty();
        }
        match mode {
            Mode::Deny => HookOutput::deny(trimmed).to_bytes(),
            Mode::Hint => HookOutput::context("PreToolUse", trimmed).to_bytes(),
        }
    }
}

/// Deserialize the tool input and extract the targeted file path.
///
/// Consumes `file_path` out of the typed input struct so the remaining
/// fields (e.g., `offset`, `limit`, `old_string`) can be reused downstream
/// for symbol resolution and threshold checks.
///
/// Note: the three-way `match kind` here mirrors
/// [`util::tool_file_path`](super::super::util::tool_file_path) but
/// cannot delegate to it — `tool_file_path` returns a cloned path,
/// whereas this function `.take()`s `file_path` out of the owned input
/// so the remaining fields stay reusable. Different mutation semantics,
/// same dispatch shape.
fn take_target_path(
    kind: ToolKind,
    input: &HookInput,
) -> Option<(String, Option<ReadToolInput>, Option<EditToolInput>)> {
    let mut read_input = (kind == ToolKind::Read)
        .then(|| input.tool_input_as::<ReadToolInput>())
        .flatten();
    let mut edit_input = input.edit_input();
    let write_input = (kind == ToolKind::Write)
        .then(|| input.tool_input_as::<WriteToolInput>())
        .flatten();

    let file_path = match kind {
        ToolKind::Read => read_input.as_mut().and_then(|r| r.file_path.take()),
        ToolKind::Edit => edit_input.as_mut().and_then(|e| e.file_path.take()),
        ToolKind::Write => write_input.and_then(|w| w.file_path),
    }?;
    Some((file_path, read_input, edit_input))
}

/// Resolve a file path into a validated interception target.
///
/// Returns `None` for paths outside the mount root, VFS companion paths,
/// files without symbol decomposition, or accesses within the grace
/// period after a previous deny. Resolves the enclosing symbol once here
/// using the cached decomposed source — no extra disk reads in
/// [`build_view`].
fn resolve_target<'a>(
    ctx: &ScriptContext<'_>,
    file_path: &'a str,
    kind: ToolKind,
    read_input: Option<ReadToolInput>,
    edit_input: Option<&EditToolInput>,
) -> Option<Target<'a>> {
    let activation = ctx.activation();
    let root = activation.root();
    let abs = Path::new(file_path);
    let rel = abs.strip_root(root)?.to_str()?;

    if super::super::resolve_companion(ctx.chain(), root, abs).is_some() {
        return None;
    }

    let decomposed = activation.decomposition_cache()?.get(Path::new(rel)).ok()?;
    if decomposed.decomposed.is_empty() {
        return None;
    }

    let fs = activation.fs();
    if util::is_within_grace(fs.as_ref(), rel) {
        return None;
    }
    util::stamp_atime(activation, rel);

    let sym = resolve_target_symbol(kind, &decomposed, read_input.as_ref(), edit_input);
    Some(Target {
        kind,
        rel,
        read_input,
        decomposed,
        sym,
    })
}

/// Resolve which symbol the tool call targets, if any.
///
/// For `Read`, uses the `offset` field. For `Edit`, locates the
/// `old_string` in the cached decomposed source (no disk read) and maps
/// the line to an enclosing symbol. `Write` never targets a specific
/// symbol.
fn resolve_target_symbol(
    kind: ToolKind,
    decomposed: &DecomposedSource,
    read_input: Option<&ReadToolInput>,
    edit_input: Option<&EditToolInput>,
) -> Option<String> {
    match kind {
        ToolKind::Read => util::resolve_symbol_at_line(
            &decomposed.decomposed,
            usize::try_from(read_input.and_then(|r| r.offset).unwrap_or(0)).unwrap_or(usize::MAX),
            &decomposed.rope,
        ),
        ToolKind::Edit => edit_input
            .and_then(|e| e.old_string.as_deref())
            .and_then(|old| util::find_line_of_string(&decomposed.source, old))
            .and_then(|line| {
                util::resolve_symbol_at_line(
                    &decomposed.decomposed,
                    usize::try_from(line).unwrap_or(usize::MAX).saturating_sub(1),
                    &decomposed.rope,
                )
            }),
        ToolKind::Write => None,
    }
}

/// Pick deny-vs-hint mode for the current tool call.
///
/// Only `Read` can be denied — `Edit`/`Write` always render as hints.
fn pick_mode(target: &Target<'_>, policy: &PreToolPolicy) -> Mode {
    if target.kind != ToolKind::Read {
        return Mode::Hint;
    }

    let narrow = target
        .read_input
        .as_ref()
        .and_then(|r| r.limit)
        .is_some_and(|l| l.cast_signed() <= policy.narrow_read_limit());
    if narrow {
        return Mode::Hint;
    }

    let total_lines = target.decomposed.source.lines().count();
    let threshold = policy.deny_lines_threshold();
    if threshold < 0 || i64::try_from(total_lines).unwrap_or(i64::MAX) < threshold {
        return Mode::Hint;
    }
    Mode::Deny
}

/// Build the Jinja rendering context for the file-access template.
fn build_view(target: &Target<'_>, mode: Mode, policy: &PreToolPolicy) -> minijinja::Value {
    let decomposed = &target.decomposed;
    let total_lines = decomposed.source.lines().count();
    let total_bytes = decomposed.source.len();
    let ext = Path::new(target.rel).extension_str().unwrap_or("");
    let config = minijinja::Value::from_serialize(policy);
    let fragments = fragment_list(&decomposed.decomposed, decomposed);
    let rel = target.rel;
    let sym = target.sym.as_deref();
    let tool_name: &str = target.kind.as_ref();

    minijinja::context! {
        mode, tool_name, rel, sym, ext, fragments, config, total_lines, total_bytes
    }
}
