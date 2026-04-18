//! LSP symlink directory resolution and target construction.
//!
//! Bridges LSP query results (locations, call hierarchy items) with the VFS
//! symlink model. Each LSP target is reverse-mapped to a symbol in the target
//! file via tree-sitter decomposition, producing symlinks like
//! `callers/init-lib.rs:42` that point to `lib.rs@/symbols/init@/body.rs`.
//! Falls back to line-slice links when decomposition is unavailable.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use color_eyre::eyre::Result;
use lsp_types::SymbolInformation;
use nyne::path_utils::PathExt;
use nyne::router::{CachePolicy, NamedNode, Node};
use nyne_companion::Companion;
use nyne_source::{
    DecomposedSource, DecompositionCache, Fragment, SourcePaths, SyntaxRegistry, find_fragment, find_fragment_at_line,
};

use crate::provider::content::{Target, actions, query_lsp_targets};
use crate::session::handle::{Handle, LspQuery};
use crate::session::manager::Manager;
use crate::session::uri::uri_to_file_path;

/// Resolved fragment context: shared decomposition, the fragment, and its LSP handle.
type FragmentContext = (Arc<DecomposedSource>, Fragment, Arc<Handle>);

/// Shared source services needed by LSP symlink resolution.
pub struct SourceCtx<'a> {
    pub(crate) syntax: &'a SyntaxRegistry,
    pub(crate) decomposition: &'a DecompositionCache,
    pub(crate) symbols_dir: &'a str,
}

/// Build a companion path to a specific symbol in a decomposed target file.
///
/// Returns `None` if the target can't be decomposed or the fragment isn't found,
/// in which case the caller should fall back to a line-slice link.
fn resolve_symbol_link(
    companion: &Companion,
    ctx: &SourceCtx<'_>,
    target_path: &Path,
    rel_path: &str,
    target_line: u32,
    base: &Path,
) -> Option<PathBuf> {
    ctx.syntax.decomposer_for(target_path)?;
    let target_shared = ctx.decomposition.get(target_path).ok()?;
    let frag_path = find_fragment_at_line(&target_shared.decomposed, target_line as usize, &target_shared.rope)?;
    let mut target = PathBuf::from(format!("{}/{}", companion.companion_name(rel_path), ctx.symbols_dir));
    for name in &frag_path {
        target.push(companion.companion_name(name));
    }
    Some(target.relative_to(base))
}

/// Build a fallback line-slice link when symbol resolution fails.
fn fallback_line_link(companion: &Companion, rel_path: &str, target_line: u32, base: &Path) -> PathBuf {
    let line_1based = target_line + 1;
    let target = PathBuf::from(format!("{}/lines:{line_1based}", companion.companion_name(rel_path)));
    target.relative_to(base)
}

/// Build the base path for the symlink directory.
///
/// Layout: `<source_file>@/symbols/<frag1>@/.../<fragN>@/<lsp_dir>`
fn build_symlink_base(
    companion: &Companion,
    source_file: &Path,
    fragment_path: &[String],
    lsp_dir: &str,
    symbols_dir: &str,
) -> PathBuf {
    let source = source_file.to_string_lossy();
    let mut base_path = format!("{}/{symbols_dir}", companion.companion_name(&source));
    for frag in fragment_path {
        base_path.push('/');
        base_path.push_str(&companion.companion_name(frag));
    }
    base_path.push('/');
    base_path.push_str(lsp_dir);
    PathBuf::from(base_path)
}

/// Build a display name for an LSP target link.
fn target_link_name(target: &Target) -> String {
    let file_basename = target
        .rel_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    let line_1based = target.line + 1;
    match &target.name {
        Some(name) => format!("{name}-{file_basename}:{line_1based}"),
        None => format!("{file_basename}:{line_1based}"),
    }
}

/// Resolve a fragment and its LSP handle from a symbol path.
///
/// Shared preamble for LSP directory resolvers: validates the file has a
/// decomposer, retrieves the decomposition, finds the fragment, and obtains
/// the LSP handle.
fn resolve_fragment_handle(
    ctx: &SourceCtx<'_>,
    lsp: &Arc<Manager>,
    source_file: &Path,
    fragment_path: &[String],
) -> Result<Option<FragmentContext>> {
    let Some(_decomposer) = ctx.syntax.decomposer_for(source_file) else {
        return Ok(None);
    };
    let shared = ctx.decomposition.get(source_file)?;
    let Some(frag) = find_fragment(&shared.decomposed, fragment_path) else {
        return Ok(None);
    };
    let frag = frag.clone();

    let Some(lsp_handle) = Handle::for_file(lsp, source_file) else {
        return Ok(None);
    };

    Ok(Some((shared, frag, lsp_handle)))
}
/// Resolve an LSP symlink directory for a symbol.
///
/// Called for paths like `file.rs@/symbols/Foo@/callers/`.
/// Queries LSP, then reverse-maps each result to a symbol in the
/// target file via tree-sitter decomposition.
pub fn resolve_lsp_symlink_dir(
    companion: &Companion,
    ctx: &SourceCtx<'_>,
    lsp: &Arc<Manager>,
    source_file: &Path,
    fragment_path: &[String],
    lsp_dir: &str,
) -> Result<Option<Vec<NamedNode>>> {
    let Some((shared, frag, lsp_handle)) = resolve_fragment_handle(ctx, lsp, source_file, fragment_path)? else {
        return Ok(None);
    };

    let targets = query_lsp_targets(
        &lsp_handle,
        &shared.source,
        frag.span.name_byte_offset,
        lsp_dir,
        &frag.line_range(&shared.rope),
    )?;

    if targets.is_empty() {
        return Ok(Some(Vec::new()));
    }

    let base = build_symlink_base(companion, source_file, fragment_path, lsp_dir, ctx.symbols_dir);
    let nodes = build_target_nodes(companion, ctx, &targets, &base);
    Ok(Some(nodes))
}

/// Convert LSP targets into deduplicated symlink nodes.
///
/// Targets carry project-relative paths (overlay root already stripped by
/// [`QueryResult::into_targets`]). Each target is reverse-mapped to a symbol
/// in the target file via tree-sitter decomposition, producing symlinks.
/// Falls back to line-slice links when decomposition is unavailable.
fn build_target_nodes(companion: &Companion, ctx: &SourceCtx<'_>, targets: &[Target], base: &Path) -> Vec<NamedNode> {
    let mut nodes = Vec::new();
    let mut seen = HashSet::new();

    for target in targets {
        let Some(rel_path) = target.rel_path.to_str() else {
            continue;
        };
        let target_path = &target.rel_path;

        let symlink_target = resolve_symbol_link(companion, ctx, target_path, rel_path, target.line, base)
            .unwrap_or_else(|| fallback_line_link(companion, rel_path, target.line, base));

        let link_name = target_link_name(target);
        if seen.insert(link_name.clone()) {
            nodes.push(Node::symlink(symlink_target).named(link_name));
        }
    }

    nodes
}

/// Resolve code actions for a symbol and build bare file nodes.
///
/// Returns the resolved actions alongside the bare `.diff` nodes so
/// callers can use [`actions::find_action_diff`] on lookup/remove.
pub fn resolve_actions_dir(
    ctx: &SourceCtx<'_>,
    lsp: &Arc<Manager>,
    source_file: &Path,
    fragment_path: &[String],
) -> Result<Option<(Vec<actions::ResolvedAction>, LspQuery)>> {
    let Some((shared, frag, lsp_handle)) = resolve_fragment_handle(ctx, lsp, source_file, fragment_path)? else {
        return Ok(None);
    };

    let sym = lsp_handle.at(&shared.source, frag.span.name_byte_offset);
    let resolved = actions::resolve_code_actions(&sym, &frag.line_range(&shared.rope));
    Ok(Some((resolved, sym)))
}

/// Convert LSP workspace symbol results into VFS symlinks.
///
/// Each symlink targets `<file>@/symbols/at-line/<line>`, which the
/// VFS resolves to the containing symbol's body. Link names use the
/// file basename to avoid path separators in filenames.
pub fn build_search_symlinks(
    symbols: &[SymbolInformation],
    source_root: &Path,
    base: &Path,
    source_paths: &SourcePaths,
) -> Vec<NamedNode> {
    let mut nodes = Vec::new();
    let mut seen = HashSet::new();

    for sym in symbols {
        let abs_path = uri_to_file_path(&sym.location.uri);
        let Some(rel_path) = abs_path.strip_prefix(source_root).ok() else {
            continue;
        };
        let Some(rel_str) = rel_path.to_str() else {
            continue;
        };

        // LSP lines are 0-based; at-line uses 1-based.
        let line = sym.location.range.start.line + 1;

        // Target: <file>@/<symbols>/<at-line>/<line>
        let target = PathBuf::from(format!("{rel_str}@/{}", source_paths.at_line(line as usize)));

        // Link name: <basename>::<symbol_name> (no slashes)
        let file = rel_path.file_name().and_then(|n| n.to_str()).unwrap_or(rel_str);
        let link_name = [file, "::", &sym.name].concat();

        // Deduplicate by link name — first occurrence wins.
        if seen.insert(link_name.clone()) {
            let (_, node) = Node::symlink(target.relative_to(base)).named(&link_name).into_parts();
            nodes.push(
                node.with_cache_policy(CachePolicy::with_ttl(Duration::ZERO))
                    .named(link_name),
            );
        }
    }

    nodes
}

#[cfg(test)]
mod tests;
