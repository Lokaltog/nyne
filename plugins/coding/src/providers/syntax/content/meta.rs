use std::io::{Error, ErrorKind};
use std::ops::Range;
use std::str::from_utf8;
use std::sync::Arc;

use color_eyre::eyre::{self, Result};
use nyne::dispatch::context::RequestContext;
use nyne::node::VirtualNode;
use nyne::node::capabilities::{Readable, Writable};
use nyne::node::kind::WriteOutcome;
use nyne::templates::TemplateHandle;
use nyne::types::vfs_path::VfsPath;

use super::FragmentResolver;
use super::overview::SymbolOverviewContent;
use crate::edit::splice::{indent_at, line_start_of, splice_validate_write};
use crate::syntax::decomposed::DecomposedSource;
use crate::syntax::fragment::{Fragment, FragmentKind, find_fragment_of_kind};
use crate::syntax::spec::{Decomposer, SpliceMode};
use crate::syntax::{self};

/// Which byte range to address in a source file — resolved lazily from
/// the current file state so byte offsets are never stale.
///
/// Used by both read paths ([`super::SourceSlice`]) and write paths
/// ([`MetaSplice`]). Read-only content types ([`SignatureContent`],
/// [`DocstringContent`], [`DecoratorsContent`]) resolve their ranges
/// from the same [`FragmentResolver`].
#[derive(Clone, Debug)]
pub(in crate::providers::syntax) enum SpliceTarget {
    /// Fragment body: `line_start_of(full_span.start)..full_span.end`.
    FragmentBody(Vec<String>),
    /// Signature text within a fragment's byte range.
    FragmentSignature(Vec<String>),
    /// Doc comment range from fragment metadata.
    FragmentDocComment(Vec<String>),
    /// Decorator/attribute range (snapped to line start).
    FragmentDecorators(Vec<String>),
    /// Import span.
    Imports,
    /// File-level doc comment (e.g. `//!` in Rust).
    FileDoc,
    /// Code block body inside a document section, identified by parent
    /// fragment path and the code block's `fs_name`.
    CodeBlockBody { parent_path: Vec<String>, fs_name: String },
}

// Readable content types — all resolve lazily via FragmentResolver

/// Readable content for the symbol's signature line.
pub(in crate::providers::syntax) struct SignatureContent {
    pub resolver: FragmentResolver,
    pub fragment_path: Vec<String>,
}

impl Readable for SignatureContent {
    fn read(&self, _ctx: &RequestContext<'_>) -> Result<Vec<u8>> {
        let shared = self.resolver.decompose()?;
        let frag = syntax::require_fragment(&shared.decomposed, &self.fragment_path)?;
        let sig = frag
            .signature
            .as_deref()
            .ok_or_else(|| eyre::eyre!("no signature on fragment {:?}", self.fragment_path))?;
        Ok(sig.as_bytes().to_vec())
    }
}

/// Readable content for the symbol's docstring (stripped of comment markers).
pub(in crate::providers::syntax) struct DocstringContent {
    pub resolver: FragmentResolver,
    pub fragment_path: Vec<String>,
}

impl Readable for DocstringContent {
    fn read(&self, _ctx: &RequestContext<'_>) -> Result<Vec<u8>> {
        let shared = self.resolver.decompose()?;
        let frag = syntax::require_fragment(&shared.decomposed, &self.fragment_path)?;
        let range = frag
            .child_of_kind(&FragmentKind::Docstring)
            .map(|c| &c.byte_range)
            .ok_or_else(|| eyre::eyre!("no doc comment on fragment {:?}", self.fragment_path))?;
        let comment = &shared.source[range.clone()];
        Ok(shared.decomposer.strip_doc_comment(comment).into_bytes())
    }
}

/// Readable content for the file-level docstring (stripped of comment markers).
pub(in crate::providers::syntax) struct FileDocstringContent {
    pub resolver: FragmentResolver,
}

impl Readable for FileDocstringContent {
    fn read(&self, _ctx: &RequestContext<'_>) -> Result<Vec<u8>> {
        let shared = self.resolver.decompose()?;
        let frag = find_fragment_of_kind(&shared.decomposed, &FragmentKind::Docstring)
            .ok_or_else(|| eyre::eyre!("no file-level doc in {}", self.resolver.source_file()))?;
        let comment = &shared.source[frag.byte_range.clone()];
        Ok(shared.decomposer.strip_doc_comment(comment).into_bytes())
    }
}

/// Readable content for the symbol's decorators/attributes.
pub(in crate::providers::syntax) struct DecoratorsContent {
    pub resolver: FragmentResolver,
    pub fragment_path: Vec<String>,
}

impl Readable for DecoratorsContent {
    fn read(&self, _ctx: &RequestContext<'_>) -> Result<Vec<u8>> {
        let shared = self.resolver.decompose()?;
        let frag = syntax::require_fragment(&shared.decomposed, &self.fragment_path)?;
        let range = frag
            .child_of_kind(&FragmentKind::Decorator)
            .map(|c| &c.byte_range)
            .ok_or_else(|| eyre::eyre!("no decorator range on fragment {:?}", self.fragment_path))?;
        let start = line_start_of(&shared.source, range.start);
        let bytes = shared.source.as_bytes().get(start..range.end).ok_or_else(|| {
            eyre::eyre!(
                "decorator range {start}..{} out of bounds for {:?}",
                range.end,
                self.fragment_path,
            )
        })?;
        Ok(bytes.to_vec())
    }
}

/// Readable content for the `lines` virtual file — reads the full source file.
///
/// Sliced variants (`lines:M-N`) are handled by the [`LineSlice`](nyne::node::line_slice::LineSlice)
/// plugin attached via `.sliceable()`.
pub(in crate::providers::syntax) struct LinesContent {
    pub source_file: VfsPath,
}

impl Readable for LinesContent {
    fn read(&self, ctx: &RequestContext<'_>) -> Result<Vec<u8>> { ctx.real_fs.read(&self.source_file) }
}

// Writable splice types

/// Validated splice write whose byte range is resolved lazily at write time.
///
/// Uses a [`FragmentResolver`] to re-derive byte offsets from the current
/// file state, so writes never target stale ranges.
pub(in crate::providers::syntax) struct MetaSplice {
    pub resolver: FragmentResolver,
    pub target: SpliceTarget,
}

impl MetaSplice {
    /// Resolve the byte range from the current file state.
    ///
    /// Re-decomposes the source file to get fresh byte offsets, then
    /// derives the target range from the current fragment tree.
    fn resolve(&self) -> Result<ResolvedSplice> {
        let shared = self.resolver.decompose()?;
        let source = &shared.source;
        let frags = &shared.decomposed;
        let byte_range = match &self.target {
            SpliceTarget::Imports => {
                let imports = find_fragment_of_kind(&shared.decomposed, &FragmentKind::Imports)
                    .ok_or_else(|| eyre::eyre!("no import span in {}", self.resolver.source_file()))?;
                let start = line_start_of(source, imports.byte_range.start);
                start..imports.byte_range.end
            }
            SpliceTarget::FileDoc => {
                let doc = find_fragment_of_kind(&shared.decomposed, &FragmentKind::Docstring)
                    .ok_or_else(|| eyre::eyre!("no file-level doc in {}", self.resolver.source_file()))?;
                let start = line_start_of(source, doc.byte_range.start);
                start..doc.byte_range.end
            }
            SpliceTarget::FragmentBody(path) => {
                let frag = syntax::require_fragment(frags, path)?;
                match shared.decomposer.splice_mode() {
                    SpliceMode::Line => {
                        let span = frag.full_span();
                        let start = line_start_of(source, span.start);
                        start..span.end
                    }
                    SpliceMode::Byte => frag.full_span(),
                }
            }
            SpliceTarget::FragmentSignature(path) => {
                let frag = syntax::require_fragment(frags, path)?;
                let sig = frag
                    .signature
                    .as_deref()
                    .ok_or_else(|| eyre::eyre!("no signature on fragment {:?}", path))?;
                find_signature_range(source, sig, frag.byte_range.start)
            }
            SpliceTarget::FragmentDocComment(path) => {
                let frag = syntax::require_fragment(frags, path)?;
                frag.child_of_kind(&FragmentKind::Docstring)
                    .map(|c| c.byte_range.clone())
                    .ok_or_else(|| eyre::eyre!("no doc comment range on fragment {path:?}"))?
            }
            SpliceTarget::FragmentDecorators(path) => {
                let frag = syntax::require_fragment(frags, path)?;
                let range = frag
                    .child_of_kind(&FragmentKind::Decorator)
                    .map(|c| &c.byte_range)
                    .ok_or_else(|| eyre::eyre!("no decorator range on fragment {path:?}"))?;
                let start = line_start_of(source, range.start);
                start..range.end
            }
            SpliceTarget::CodeBlockBody { parent_path, fs_name } => {
                let parent = syntax::require_fragment(frags, parent_path)?;
                let cb = parent
                    .children
                    .iter()
                    .find(|c| c.fs_name.as_deref() == Some(fs_name.as_str()))
                    .ok_or_else(|| eyre::eyre!("code block {fs_name:?} not found in {parent_path:?}"))?;
                cb.byte_range.clone()
            }
        };
        Ok(ResolvedSplice { shared, byte_range })
    }

    /// Splice `new_content` into the source file, validate, write back, and
    /// invalidate the decomposition cache.
    pub(super) fn splice_write(&self, ctx: &RequestContext<'_>, new_content: &str) -> Result<WriteOutcome> {
        let resolved = self.resolve()?;
        let n = splice_validate_write(
            ctx.real_fs,
            self.resolver.source_file(),
            resolved.byte_range,
            new_content,
            |spliced| resolved.shared.decomposer.validate(spliced).map_err(|e| e.to_string()),
        )?;
        self.resolver.invalidate();
        Ok(WriteOutcome::Written(n))
    }

    /// Handle truncate-write: if `data` is empty, remove the span entirely;
    /// otherwise delegate to `write_fn`.
    pub(super) fn truncate_or(
        &self,
        ctx: &RequestContext<'_>,
        data: &[u8],
        write_fn: impl FnOnce(&str) -> Result<WriteOutcome>,
    ) -> Result<WriteOutcome> {
        let text = from_utf8(data)?;
        if text.is_empty() {
            self.splice_write(ctx, "")
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

impl Writable for MetaSplice {
    fn write(&self, ctx: &RequestContext<'_>, data: &[u8]) -> Result<WriteOutcome> {
        self.splice_write(ctx, from_utf8(data)?)
    }
}

/// Writable splice for the symbol's docstring.
///
/// Accepts plain text (no comment markers), wraps with language-specific
/// doc comment syntax, and splices into the source file.
pub(in crate::providers::syntax) struct DocstringSplice {
    pub meta: MetaSplice,
}

impl DocstringSplice {
    fn wrap_and_splice(&self, ctx: &RequestContext<'_>, plain: &str) -> Result<WriteOutcome> {
        let resolved = self.meta.resolve()?;
        let indent = indent_at(&resolved.shared.source, resolved.byte_range.start);
        let wrapped = resolved.shared.decomposer.wrap_doc_comment(plain, indent);
        self.meta.splice_write(ctx, &wrapped)
    }
}

impl Writable for DocstringSplice {
    fn write(&self, ctx: &RequestContext<'_>, data: &[u8]) -> Result<WriteOutcome> {
        self.wrap_and_splice(ctx, from_utf8(data)?)
    }

    fn truncate_write(&self, ctx: &RequestContext<'_>, data: &[u8]) -> Result<WriteOutcome> {
        self.meta
            .truncate_or(ctx, data, |plain| self.wrap_and_splice(ctx, plain))
    }
}

/// Writable splice for the file-level docstring.
///
/// Accepts plain text, wraps with file-level doc comment syntax
/// (e.g. `//!` in Rust), and splices into the source file.
pub(in crate::providers::syntax) struct FileDocstringSplice {
    pub meta: MetaSplice,
}

impl FileDocstringSplice {
    fn wrap_and_splice(&self, ctx: &RequestContext<'_>, plain: &str) -> Result<WriteOutcome> {
        let resolved = self.meta.resolve()?;
        let indent = indent_at(&resolved.shared.source, resolved.byte_range.start);
        let wrapped = resolved.shared.decomposer.wrap_file_doc_comment(plain, indent);
        self.meta.splice_write(ctx, &wrapped)
    }
}

impl Writable for FileDocstringSplice {
    fn write(&self, ctx: &RequestContext<'_>, data: &[u8]) -> Result<WriteOutcome> {
        self.wrap_and_splice(ctx, from_utf8(data)?)
    }

    fn truncate_write(&self, ctx: &RequestContext<'_>, data: &[u8]) -> Result<WriteOutcome> {
        self.meta
            .truncate_or(ctx, data, |plain| self.wrap_and_splice(ctx, plain))
    }
}

/// Writable splice for the symbol's decorators/attributes.
pub(in crate::providers::syntax) struct DecoratorsSplice {
    pub meta: MetaSplice,
}

impl Writable for DecoratorsSplice {
    fn write(&self, ctx: &RequestContext<'_>, data: &[u8]) -> Result<WriteOutcome> {
        self.meta.splice_write(ctx, from_utf8(data)?)
    }

    fn truncate_write(&self, ctx: &RequestContext<'_>, data: &[u8]) -> Result<WriteOutcome> {
        self.meta
            .truncate_or(ctx, data, |content| self.meta.splice_write(ctx, content))
    }
}

/// Writable content for the `lines` virtual file — replaces the full source file.
///
/// Validates the new content with the decomposer before writing.
/// Sliced writes (`lines:M-N`) are handled by the [`LineSlice`](nyne::node::line_slice::LineSlice)
/// plugin's `SlicedWritable`, which splices and delegates back through this writable.
pub(in crate::providers::syntax) struct LinesWrite {
    pub source_file: VfsPath,
    pub decomposer: Arc<dyn Decomposer>,
    pub resolver: FragmentResolver,
}

impl Writable for LinesWrite {
    fn write(&self, ctx: &RequestContext<'_>, data: &[u8]) -> Result<WriteOutcome> {
        let new_content = from_utf8(data)?;
        self.decomposer
            .validate(new_content)
            .map_err(|e| Error::new(ErrorKind::InvalidInput, e.to_string()))?;
        ctx.real_fs.write(&self.source_file, data)?;
        self.resolver.invalidate();
        Ok(WriteOutcome::Written(data.len()))
    }
}

// Node construction

/// Build per-symbol meta-file nodes from a fragment.
pub(in crate::providers::syntax) fn build_meta_nodes(
    frag: &Fragment,
    ext: &str,
    overview_handle: &TemplateHandle,
    resolver: &FragmentResolver,
    fragment_path: &[String],
) -> Vec<VirtualNode> {
    use crate::providers::names::{FILE_DECORATORS, FILE_DOCSTRING, FILE_OVERVIEW, FILE_SIGNATURE};
    use crate::providers::syntax::newline;

    let mut nodes = Vec::new();
    let path = fragment_path.to_vec();

    // signature.<ext> — only if the fragment has a signature.
    if frag.signature.is_some() {
        let name = format!("{FILE_SIGNATURE}.{ext}");
        let node = VirtualNode::file(&name, SignatureContent {
            resolver: resolver.clone(),
            fragment_path: path.clone(),
        })
        .with_writable(MetaSplice {
            resolver: resolver.clone(),
            target: SpliceTarget::FragmentSignature(path.clone()),
        });
        nodes.push(newline::with_newline_middlewares(node));
    }

    // docstring.txt — only for fragments with a docstring child.
    if frag.child_of_kind(&FragmentKind::Docstring).is_some() {
        let node = VirtualNode::file(FILE_DOCSTRING, DocstringContent {
            resolver: resolver.clone(),
            fragment_path: path.clone(),
        })
        .with_writable(DocstringSplice {
            meta: MetaSplice {
                resolver: resolver.clone(),
                target: SpliceTarget::FragmentDocComment(path.clone()),
            },
        });
        nodes.push(newline::with_newline_middlewares(node));
    }

    // decorators.<ext> — only for fragments with a decorator child.
    if frag.child_of_kind(&FragmentKind::Decorator).is_some() {
        let name = format!("{FILE_DECORATORS}.{ext}");
        let node = VirtualNode::file(&name, DecoratorsContent {
            resolver: resolver.clone(),
            fragment_path: path.clone(),
        })
        .with_writable(DecoratorsSplice {
            meta: MetaSplice {
                resolver: resolver.clone(),
                target: SpliceTarget::FragmentDecorators(path.clone()),
            },
        });
        nodes.push(newline::with_newline_middlewares(node));
    }

    // OVERVIEW.md — only if the fragment has children.
    if !frag.children.is_empty() {
        nodes.push(overview_handle.node(FILE_OVERVIEW, SymbolOverviewContent {
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
        frag_start..frag_start
    }
}
