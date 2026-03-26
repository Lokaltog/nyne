//! Content reading, writing, and rendering for decomposed symbols.
//!
//! Each submodule handles a different aspect of symbol content: body splicing,
//! meta-files (signature, docstring, decorators), LSP features, overview tables,
//! rename/delete previews, and code-analysis hints.

/// LSP code action resolution and application.
pub(super) mod actions;
/// Symbol deletion with surrounding whitespace cleanup.
pub(super) mod delete;
/// Inlay hints content and analysis rendering.
pub(super) mod hints;
/// LSP feature nodes, diagnostics, and view rendering.
pub(super) mod lsp;
/// Symbol meta-file rendering and splicing (signature, docstring, decorators).
pub(super) mod meta;
/// OVERVIEW.md rendering for symbol tables.
pub(super) mod overview;
/// LSP-powered symbol rename and file rename previews.
pub(super) mod rename;
/// Symbol body write and splice operations.
mod write;

use color_eyre::eyre::{self, Result, eyre};
use nyne::dispatch::context::RequestContext;
use nyne::node::capabilities::Readable;

// Re-exports for the parent module.
pub(super) use self::{
    meta::{
        FileDocstringContent, FileDocstringSplice, LinesContent, LinesWrite, MetaSplice, SpliceTarget, build_meta_nodes,
    },
    overview::{FileOverviewContent, OverviewContent},
    write::BodySplice,
};
use crate::edit::splice::{line_end_of, line_start_of};
pub(super) use crate::providers::fragment_resolver::FragmentResolver;
use crate::syntax;
use crate::syntax::fragment::{FragmentKind, find_fragment_of_kind};
use crate::syntax::spec::SpliceMode;

/// Readable content that returns a byte range of the original source.
///
/// Re-derives the byte range from a fresh decomposition on every read so
/// that content is never stale after writes.
///
/// When the decomposer uses [`SpliceMode::Byte`], the range covers full
/// lines but bytes outside the exact fragment span are replaced with spaces.
/// This keeps column alignment intact while extracting only the target
/// symbol — essential for Lisp-family languages where multiple expressions
/// share a line.
pub(super) struct SourceSlice {
    pub resolver: FragmentResolver,
    pub target: SpliceTarget,
}

/// [`Readable`] implementation for [`SourceSlice`].
impl Readable for SourceSlice {
    /// Read the targeted byte range from the source, applying byte-masking if needed.
    fn read(&self, _ctx: &RequestContext<'_>) -> Result<Vec<u8>> {
        let shared = self.resolver.decompose()?;
        let frags = &shared.decomposed;

        let (byte_range, mask_span) = match &self.target {
            SpliceTarget::FragmentBody(path) => {
                let frag = syntax::require_fragment(frags, path)?;
                let span = frag.full_span();
                let body_start = line_start_of(&shared.source, span.start);
                match shared.decomposer.splice_mode() {
                    SpliceMode::Line => (body_start..span.end, None),
                    SpliceMode::Byte => {
                        let body_end = line_end_of(&shared.source, span.end);
                        (body_start..body_end, Some(span))
                    }
                }
            }
            SpliceTarget::Imports => {
                let imports = find_fragment_of_kind(&shared.decomposed, &FragmentKind::Imports)
                    .ok_or_else(|| eyre!("no import span in {}", self.resolver.source_file()))?;
                let start = line_start_of(&shared.source, imports.byte_range.start);
                (start..imports.byte_range.end, None)
            }
            SpliceTarget::CodeBlockBody { parent_path, fs_name } => {
                let parent = syntax::require_fragment(frags, parent_path)?;
                let cb = parent
                    .child_by_fs_name(fs_name)
                    .ok_or_else(|| eyre!("code block {fs_name:?} not found in {parent_path:?}"))?;
                (cb.byte_range.clone(), None)
            }
            // Other SpliceTarget variants are handled by their own Readable types
            // (SignatureContent, DocstringContent, DecoratorsContent).
            target => eyre::bail!("SourceSlice does not handle {target:?}"),
        };

        let slice = shared.source.as_bytes().get(byte_range.clone()).unwrap_or(&[]);

        let Some(mask) = mask_span else {
            return Ok(slice.to_vec());
        };

        // Byte-masking: replace bytes outside the exact span with spaces,
        // preserving newlines so line structure is maintained.
        let mut buf = slice.to_vec();
        let base = byte_range.start;
        let mask_start = mask.start.saturating_sub(base);
        let mask_end = mask.end.saturating_sub(base).min(buf.len());

        for byte in buf.get_mut(..mask_start).unwrap_or_default() {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
        for byte in buf.get_mut(mask_end..).unwrap_or_default() {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
        Ok(buf)
    }
}
