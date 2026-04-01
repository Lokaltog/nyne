//! Symbol meta-file rendering and splicing (signature, docstring, decorators).
//!
//! Each meta-file is a [`Readable`]/[`Writable`] pair that lazily resolves byte
//! ranges from a fresh decomposition on every access, ensuring content is never
//! stale after writes. Writes are validated by tree-sitter before being committed.

use std::io::{Error, ErrorKind};
use std::ops::{Deref, Range};
use std::path::PathBuf;
use std::str::from_utf8;
use std::sync::Arc;

use color_eyre::eyre::{self, Result};
use nyne::router::{
    AffectedFiles, Filesystem, NamedNode, ReadContext, Readable, Writable, WriteContext, lazy_slice_node,
};
use nyne::templates::TemplateHandle;

use super::FragmentResolver;
use super::overview::SymbolOverviewContent;
use crate::edit::splice::{indent_at_rope, line_start_of_rope, splice_validate_write};
use crate::plugin::config::vfs::VfsFiles;
use crate::syntax::decomposed::DecomposedSource;
use crate::syntax::fragment::{Fragment, FragmentKind, find_fragment_of_kind};
use crate::syntax::spec::{Decomposer, SpliceMode};
use crate::syntax::{self};

/// Shared, cheaply cloneable path identifying a fragment in the decomposed tree.
///
/// Wraps `Arc<[String]>` so that multiple content readers and splice writers
/// can share the same allocation instead of cloning `Vec<String>` per node.
#[derive(Clone, Debug)]
pub(in crate::provider::syntax) struct FragmentPath(pub Arc<[String]>);

/// Construction for [`FragmentPath`].
impl FragmentPath {
    /// Create a new `FragmentPath` from a slice of path segments.
    pub fn new(segments: &[String]) -> Self { Self(Arc::from(segments)) }
}

/// Deref to `[String]` so callers can slice and iterate path segments directly.
impl Deref for FragmentPath {
    type Target = [String];

    fn deref(&self) -> &[String] { &self.0 }
}

/// Which byte range to address in a source file — resolved lazily from
/// the current file state so byte offsets are never stale.
///
/// Used by both read paths ([`super::Slice`]) and write paths
/// ([`MetaSplice`]). Read-only content types ([`SignatureContent`],
/// [`DocstringContent`], [`DecoratorsContent`]) resolve their ranges
/// from the same [`FragmentResolver`].
#[derive(Clone, Debug)]
pub(in crate::provider::syntax) enum SpliceTarget {
    /// Fragment body: `line_start_of(full_span.start)..full_span.end`.
    FragmentBody(FragmentPath),
    /// Signature text within a fragment's byte range.
    FragmentSignature(FragmentPath),
    /// Doc comment range from fragment metadata.
    FragmentDocComment(FragmentPath),
    /// Decorator/attribute range (snapped to line start).
    FragmentDecorators(FragmentPath),
    /// Import span.
    Imports,
    /// File-level doc comment (e.g. `//!` in Rust).
    FileDoc,
    /// Code block body inside a document section, identified by parent
    /// fragment path and the code block's `fs_name`.
    CodeBlockBody { parent_path: FragmentPath, fs_name: String },
}

// Readable content types — all resolve lazily via FragmentResolver

/// Readable content for the `lines` virtual file — reads the full source file.
///
/// Sliced variants (`lines:M-N`) are handled by the [`LineSlice`](nyne::node::line_slice::LineSlice)
/// plugin attached via `.sliceable()`.
pub(in crate::provider::syntax) struct LinesContent {
    pub source_file: PathBuf,
}

/// [`Readable`] implementation for [`LinesContent`].
impl Readable for LinesContent {
    /// Read the full source file content.
    fn read(&self, ctx: &ReadContext<'_>) -> Result<Vec<u8>> { ctx.fs.read_file(&self.source_file) }
}

// Writable splice types

/// Validated splice write whose byte range is resolved lazily at write time.
///
/// Uses a [`FragmentResolver`] to re-derive byte offsets from the current
/// file state, so writes never target stale ranges.
pub(in crate::provider::syntax) struct MetaSplice {
    pub resolver: FragmentResolver,
    pub target: SpliceTarget,
}

/// Methods for [`MetaSplice`].
impl MetaSplice {
    /// Resolve the byte range from the current file state.
    ///
    /// Re-decomposes the source file to get fresh byte offsets, then
    /// derives the target range from the current fragment tree.
    fn resolve(&self) -> Result<ResolvedSplice> {
        /// Extend a byte range backward to the start of its first line.
        fn line_aligned(rope: &crop::Rope, range: Range<usize>) -> Range<usize> {
            line_start_of_rope(rope, range.start)..range.end
        }

        let shared = self.resolver.decompose()?;
        let rope = crop::Rope::from(shared.source.as_str());
        let frags = &shared.decomposed;
        let resolve_frag = |path: &[String]| -> Result<&Fragment> { syntax::require_fragment(frags, path) };
        let byte_range = match &self.target {
            SpliceTarget::Imports => {
                let frag = find_fragment_of_kind(frags, &FragmentKind::Imports)
                    .ok_or_else(|| eyre::eyre!("no import span in {}", self.resolver.source_file().display()))?;
                line_aligned(&rope, frag.byte_range.clone())
            }
            SpliceTarget::FileDoc => {
                let frag = find_fragment_of_kind(frags, &FragmentKind::Docstring)
                    .ok_or_else(|| eyre::eyre!("no file-level doc in {}", self.resolver.source_file().display()))?;
                line_aligned(&rope, frag.byte_range.clone())
            }
            SpliceTarget::FragmentBody(path) => {
                let frag = resolve_frag(path)?;
                match shared.decomposer.splice_mode() {
                    SpliceMode::Line => line_aligned(&rope, frag.full_span()),
                    SpliceMode::Byte => frag.full_span(),
                }
            }
            SpliceTarget::FragmentSignature(path) => {
                let frag = resolve_frag(path)?;
                let sig = frag
                    .signature
                    .as_deref()
                    .ok_or_else(|| eyre::eyre!("no signature on fragment {:?}", &**path))?;
                find_signature_range(&shared.source, sig, frag.byte_range.start)
            }
            SpliceTarget::FragmentDocComment(path) => resolve_frag(path)?
                .child_of_kind(&FragmentKind::Docstring)
                .map(|c| c.byte_range.clone())
                .ok_or_else(|| eyre::eyre!("no doc comment range on fragment {:?}", &**path))?,
            SpliceTarget::FragmentDecorators(path) => {
                let child = resolve_frag(path)?
                    .child_of_kind(&FragmentKind::Decorator)
                    .ok_or_else(|| eyre::eyre!("no decorator range on fragment {:?}", &**path))?;
                line_aligned(&rope, child.byte_range.clone())
            }
            SpliceTarget::CodeBlockBody { parent_path, fs_name } => resolve_frag(parent_path)?
                .child_by_fs_name(fs_name)
                .ok_or_else(|| eyre::eyre!("code block {fs_name:?} not found in {:?}", &**parent_path))?
                .byte_range
                .clone(),
        };
        Ok(ResolvedSplice { shared, byte_range })
    }

    /// Splice `new_content` into the source file, validate, write back, and
    /// invalidate the decomposition cache.
    pub(super) fn splice_write(&self, fs: &dyn Filesystem, new_content: &str) -> Result<AffectedFiles> {
        let resolved = self.resolve()?;
        let source_file = self.resolver.source_file().to_owned();
        splice_validate_write(fs, &source_file, resolved.byte_range, new_content, |spliced| {
            resolved.shared.decomposer.validate(spliced).map_err(|e| e.to_string())
        })?;
        self.resolver.invalidate();
        Ok(vec![source_file])
    }

    /// Resolve the splice target, apply a wrapping function to the plain text
    /// (e.g. doc comment markers), then splice the result into the source file.
    pub(super) fn wrap_and_splice(
        &self,
        fs: &dyn Filesystem,
        plain: &str,
        wrap_fn: impl FnOnce(&dyn Decomposer, &str, &str) -> String,
    ) -> Result<AffectedFiles> {
        let resolved = self.resolve()?;
        let rope = crop::Rope::from(resolved.shared.source.as_str());
        let indent = indent_at_rope(&resolved.shared.source, &rope, resolved.byte_range.start);
        let wrapped = wrap_fn(resolved.shared.decomposer.as_ref(), plain, indent);
        self.splice_write(fs, &wrapped)
    }

    /// Handle truncate-write: if `data` is empty, remove the span entirely;
    /// otherwise delegate to `write_fn`.
    pub(super) fn truncate_or(
        &self,
        fs: &dyn Filesystem,
        data: &[u8],
        write_fn: impl FnOnce(&str) -> Result<AffectedFiles>,
    ) -> Result<AffectedFiles> {
        let text = from_utf8(data)?;
        if text.is_empty() {
            self.splice_write(fs, "")
        } else {
            write_fn(text)
        }
    }
}

/// Result of resolving a [`SpliceTarget`] against the current file state.
struct ResolvedSplice {
    shared: Arc<DecomposedSource>,
    byte_range: Range<usize>,
}

/// [`Writable`] implementation for [`MetaSplice`].
impl Writable for MetaSplice {
    /// Splice the data into the source file at the resolved byte range.
    fn write(&self, ctx: &WriteContext<'_>, data: &[u8]) -> Result<AffectedFiles> {
        self.splice_write(ctx.fs, from_utf8(data)?)
    }
}

/// Writable splice for the symbol's docstring.
///
/// Accepts plain text (no comment markers), wraps with language-specific
/// doc comment syntax, and splices into the source file.
pub(in crate::provider::syntax) struct DocstringSplice {
    pub meta: MetaSplice,
}

/// [`Writable`] implementation for [`DocstringSplice`].
impl Writable for DocstringSplice {
    /// Wrap plain text in doc comment syntax and splice into source.
    /// Empty data removes the docstring entirely.
    fn write(&self, ctx: &WriteContext<'_>, data: &[u8]) -> Result<AffectedFiles> {
        self.meta.truncate_or(ctx.fs, data, |plain| {
            self.meta
                .wrap_and_splice(ctx.fs, plain, |d, text, indent| d.wrap_doc_comment(text, indent))
        })
    }
}

/// Writable splice for the file-level docstring.
///
/// Accepts plain text, wraps with file-level doc comment syntax
/// (e.g. `//!` in Rust), and splices into the source file.
pub(in crate::provider::syntax) struct FileDocstringSplice {
    pub meta: MetaSplice,
}

/// [`Writable`] implementation for [`FileDocstringSplice`].
impl Writable for FileDocstringSplice {
    /// Wrap plain text in file doc comment syntax and splice into source.
    /// Empty data removes the file docstring entirely.
    fn write(&self, ctx: &WriteContext<'_>, data: &[u8]) -> Result<AffectedFiles> {
        self.meta.truncate_or(ctx.fs, data, |plain| {
            self.meta
                .wrap_and_splice(ctx.fs, plain, |d, text, indent| d.wrap_file_doc_comment(text, indent))
        })
    }
}

/// Writable splice for the symbol's decorators/attributes.
pub(in crate::provider::syntax) struct DecoratorsSplice {
    pub meta: MetaSplice,
}

/// [`Writable`] implementation for [`DecoratorsSplice`].
impl Writable for DecoratorsSplice {
    /// Splice new decorator content into the source file.
    /// Empty data removes the decorators entirely.
    fn write(&self, ctx: &WriteContext<'_>, data: &[u8]) -> Result<AffectedFiles> {
        self.meta
            .truncate_or(ctx.fs, data, |content| self.meta.splice_write(ctx.fs, content))
    }
}

/// Writable content for the `lines` virtual file — replaces the full source file.
///
/// Validates the new content with the decomposer before writing.
/// Sliced writes (`lines:M-N`) are handled by the [`LineSlice`](nyne::node::line_slice::LineSlice)
/// plugin's `SlicedWritable`, which splices and delegates back through this writable.
pub(in crate::provider::syntax) struct LinesWrite {
    pub source_file: PathBuf,
    pub decomposer: Arc<dyn Decomposer>,
    pub resolver: FragmentResolver,
}

/// [`Writable`] implementation for [`LinesWrite`].
impl Writable for LinesWrite {
    /// Write full source file content, replacing the existing file.
    fn write(&self, ctx: &WriteContext<'_>, data: &[u8]) -> Result<AffectedFiles> {
        let new_content = from_utf8(data)?;
        self.decomposer
            .validate(new_content)
            .map_err(|e| Error::new(ErrorKind::InvalidInput, e.to_string()))?;
        let affected = ctx.fs.write_file(&self.source_file, data)?;
        self.resolver.invalidate();
        Ok(affected)
    }
}

// Node construction

/// Build a file-level docstring node (readable + writable).
///
/// The returned node lazily reads the file docstring and strips comment markers.
/// Used by both the route-tree builder and the readdir inventory handler.
pub(in crate::provider::syntax) fn file_docstring_node(resolver: &FragmentResolver, files: &VfsFiles) -> NamedNode {
    let r = resolver.clone();
    lazy_slice_node(
        format!("{}.txt", files.docstring),
        move |_ctx| {
            let shared = r.decompose()?;
            let range = find_fragment_of_kind(&shared.decomposed, &FragmentKind::Docstring)
                .ok_or_else(|| eyre::eyre!("no file-level doc in {}", r.source_file().display()))?
                .byte_range
                .clone();
            Ok(shared.decomposer.strip_doc_comment(&shared.source[range]).into_bytes())
        },
        FileDocstringSplice {
            meta: MetaSplice {
                resolver: resolver.clone(),
                target: SpliceTarget::FileDoc,
            },
        },
    )
}

/// Build per-symbol meta-file nodes from a fragment.
///
/// Conditionally emits `signature.<ext>`, `docstring.txt`, `decorators.<ext>`,
/// and `OVERVIEW.md` depending on which metadata the fragment carries. Each
/// node is both readable (via [`Slice`]) and writable (via [`MetaSplice`]),
/// with trailing-newline decoration for editor compatibility.
pub(in crate::provider::syntax) fn build_meta_nodes(
    frag: &Fragment,
    ext: &str,
    overview_handle: &TemplateHandle,
    resolver: &FragmentResolver,
    fragment_path: &[String],
    files: &VfsFiles,
) -> Vec<NamedNode> {
    /// Build a readable+writable meta-file node with newline decorators.
    fn meta_node(
        name: impl Into<String>,
        read_fn: impl for<'a> Fn(&ReadContext<'a>) -> Result<Vec<u8>> + Send + Sync + 'static,
        resolver: &FragmentResolver,
        target: SpliceTarget,
    ) -> NamedNode {
        lazy_slice_node(name, read_fn, MetaSplice {
            resolver: resolver.clone(),
            target,
        })
    }

    let path = FragmentPath::new(fragment_path);
    let mut nodes = Vec::new();

    // signature.<ext> — only if the fragment has a signature.
    if frag.signature.is_some() {
        let r = resolver.clone();
        let p = path.clone();
        nodes.push(meta_node(
            format!("{}.{ext}", files.signature),
            move |_ctx| {
                let shared = r.decompose()?;
                let frag = syntax::require_fragment(&shared.decomposed, &p)?;
                let sig = frag
                    .signature
                    .as_deref()
                    .ok_or_else(|| eyre::eyre!("no signature on fragment {:?}", p))?;
                Ok(sig.as_bytes().to_vec())
            },
            resolver,
            SpliceTarget::FragmentSignature(path.clone()),
        ));
    }

    // docstring.txt — only for fragments with a docstring child.
    if frag.child_of_kind(&FragmentKind::Docstring).is_some() {
        let r = resolver.clone();
        let p = path.clone();
        nodes.push(lazy_slice_node(
            format!("{}.txt", files.docstring),
            move |_ctx| {
                let shared = r.decompose()?;
                let frag = syntax::require_fragment(&shared.decomposed, &p)?;
                let range = frag
                    .child_of_kind(&FragmentKind::Docstring)
                    .map(|c| &c.byte_range)
                    .ok_or_else(|| eyre::eyre!("no doc comment on fragment {:?}", p))?;
                let comment = &shared.source[range.clone()];
                Ok(shared.decomposer.strip_doc_comment(comment).into_bytes())
            },
            DocstringSplice {
                meta: MetaSplice {
                    resolver: resolver.clone(),
                    target: SpliceTarget::FragmentDocComment(path.clone()),
                },
            },
        ));
    }

    // decorators.<ext> — only for fragments with a decorator child.
    if frag.child_of_kind(&FragmentKind::Decorator).is_some() {
        let r = resolver.clone();
        let p = path.clone();
        nodes.push(lazy_slice_node(
            format!("{}.{ext}", files.decorators),
            move |_ctx| {
                let shared = r.decompose()?;
                let frag = syntax::require_fragment(&shared.decomposed, &p)?;
                let range = frag
                    .child_of_kind(&FragmentKind::Decorator)
                    .map(|c| &c.byte_range)
                    .ok_or_else(|| eyre::eyre!("no decorator range on fragment {:?}", p))?;
                let start = line_start_of_rope(&crop::Rope::from(shared.source.as_str()), range.start);
                let bytes =
                    shared.source.as_bytes().get(start..range.end).ok_or_else(|| {
                        eyre::eyre!("decorator range {start}..{} out of bounds for {:?}", range.end, p,)
                    })?;
                Ok(bytes.to_vec())
            },
            DecoratorsSplice {
                meta: MetaSplice {
                    resolver: resolver.clone(),
                    target: SpliceTarget::FragmentDecorators(path.clone()),
                },
            },
        ));
    }

    // OVERVIEW.md — only if the fragment has children.
    if !frag.children.is_empty() {
        nodes.push(overview_handle.named_node(&files.overview, SymbolOverviewContent {
            resolver: resolver.clone(),
            fragment_path: path,
        }));
    }

    nodes
}

/// Find the byte range of a signature string within the fragment's byte range.
///
/// The signature is the first occurrence of the signature text after the
/// fragment start. Falls back to a zero-width range at fragment start if
/// not found (shouldn't happen with valid decomposition).
fn find_signature_range(source: &str, signature: &str, frag_start: usize) -> Range<usize> {
    if let Some(offset) = source[frag_start..].find(signature) {
        let start = frag_start + offset;
        start..start + signature.len()
    } else {
        tracing::warn!(
            signature,
            frag_start,
            "signature text not found in fragment range — falling back to zero-width range"
        );
        frag_start..frag_start
    }
}
