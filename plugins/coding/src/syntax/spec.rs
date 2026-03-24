//! Language specification trait and generic decomposer.

use std::marker::PhantomData;
use std::ops::Range;

use color_eyre::eyre::Result;

use super::extract::extract_fragments;
use super::fragment::{ConflictSet, DecomposedFile, Fragment, Resolution, SymbolKind};
use super::fs_mapping::{ConflictStrategy, NamingStrategy, apply_fs_mapping, resolve_conflicts};
use super::parser::{TreeSitterParser, TsNode, merge_preceding_sibling_ranges};

/// How fragment bodies are sliced from the source file for reading and
/// spliced back on write.
///
/// Most languages occupy complete lines — each symbol starts at the beginning
/// of a line and ends at the end of one. Lisp-family languages break this
/// assumption: a single line may open or close multiple S-expressions, so
/// line-based slicing would capture parts of adjacent symbols.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpliceMode {
    /// Snap to line boundaries on both read and write.
    ///
    /// This is the default for most languages. The body starts at the
    /// beginning of the line containing `full_span.start` and ends at
    /// `full_span.end`. Writes splice at the same line-snapped range.
    #[default]
    Line,

    /// Read with line-boundary snapping + byte masking; write at exact byte
    /// boundaries.
    ///
    /// On **read**, the body is snapped to full lines, but bytes outside the
    /// exact `full_span` are replaced with spaces. This preserves column
    /// alignment while extracting only the target symbol.
    ///
    /// On **write**, leading/trailing whitespace is stripped from the incoming
    /// content and the result is spliced at the exact `full_span` byte range
    /// (no line-start snapping).
    Byte,
}

/// Information extracted from a wrapper node (e.g. `export_statement`,
/// `decorated_definition`).
#[derive(Default)]
pub struct WrapperInfo {
    /// Visibility implied by the wrapper (e.g. `"export"` for `export_statement`).
    pub visibility: Option<String>,
    /// Byte range of decorator/attribute nodes within the wrapper.
    pub decorator_range: Option<Range<usize>>,
}

/// Pure language knowledge — constants, symbol mapping, and extraction methods.
///
/// Each supported language implements this trait once. [`CodeDecomposer<L>`]
/// then provides the [`Decomposer`] impl generically. Methods have defaults
/// for everything optional — languages override only what they need, all in
/// one `impl` block.
pub trait LanguageSpec: Send + Sync + 'static {
    /// Human-readable language name (e.g. `"Rust"`, `"Python"`).
    const NAME: &'static str;

    /// File extensions this language handles (e.g. `&["rs"]`, `&["ts", "tsx"]`).
    const EXTENSIONS: &'static [&'static str];

    /// Tree-sitter node kinds that represent import statements.
    const IMPORT_KINDS: &'static [&'static str];

    /// Node kinds whose children should be recursed into.
    const RECURSABLE_KINDS: &'static [&'static str];

    /// The field name used to find the body node for recursion.
    const BODY_FIELD: &'static str = "body";

    /// FS naming strategy for this language.
    const NAMING_STRATEGY: NamingStrategy = NamingStrategy::Identity;

    /// Conflict resolution strategy for this language.
    const CONFLICT_STRATEGY: ConflictStrategy = ConflictStrategy::KindSuffix;

    /// How fragment bodies are sliced from source and spliced back on write.
    const SPLICE_MODE: SpliceMode = SpliceMode::Line;

    /// Tree-sitter comment node kind for doc range extraction. `None` disables default extraction.
    const DOC_COMMENT_KIND: Option<&'static str> = None;
    /// Prefixes that identify doc comments (e.g. `&["///"]` for Rust, `&["#"]` for Nix).
    const DOC_COMMENT_PREFIXES: &'static [&'static str] = &[];
    /// Node kinds to skip when scanning for doc comments (e.g. `&["attribute_item"]` for Rust).
    const DOC_COMMENT_SKIP_KINDS: &'static [&'static str] = &[];

    /// Return the tree-sitter grammar for a given extension.
    ///
    /// Most languages return a single grammar. TypeScript returns different
    /// grammars for `.ts` vs `.tsx`.
    fn grammar(ext: &str) -> tree_sitter::Language;

    /// Map a tree-sitter node kind string to a [`SymbolKind`].
    ///
    /// Languages using `extract_custom` can leave this as the default (no
    /// mappings).
    fn map_symbol_kind(_node_kind: &str) -> Option<SymbolKind> { None }

    /// Unwrap a wrapper node (e.g. `export_statement`, `decorated_definition`)
    /// to get the inner declaration and wrapper metadata.
    ///
    /// Returns `None` if the node is not a wrapper.
    fn unwrap_wrapper(_node: TsNode<'_>) -> Option<(TsNode<'_>, WrapperInfo)> { None }

    /// Build a one-line signature string for a symbol.
    ///
    /// Default returns the first line of the node's text. Languages should
    /// override for kind-specific formatting.
    fn build_signature(node: TsNode<'_>, _kind: SymbolKind) -> String { node.first_line().to_owned() }

    /// Extract the canonical name from a symbol node.
    ///
    /// Default: reads the `name` field child.
    fn extract_name(node: TsNode<'_>, _kind: SymbolKind) -> String {
        node.field_text("name").unwrap_or("anonymous").to_owned()
    }

    /// Handle non-standard nodes that fall through the symbol mapping and
    /// wrapper unwrapping (e.g. Python assignments).
    fn extract_extra(_node: TsNode<'_>, _remaining_depth: usize, _parent_name: Option<&str>) -> Option<Fragment> {
        None
    }

    /// Bypass the standard extraction loop entirely.
    ///
    /// When this returns `Some`, the standard symbol-mapping loop is skipped
    /// and the returned fragments are used directly. Used by markdown and
    /// other non-code languages.
    fn extract_custom(_root: TsNode<'_>, _max_depth: usize) -> Option<Vec<Fragment>> { None }

    /// Extract the doc comment range for a symbol node.
    ///
    /// Default: uses [`DOC_COMMENT_KIND`](Self::DOC_COMMENT_KIND) /
    /// [`DOC_COMMENT_PREFIXES`](Self::DOC_COMMENT_PREFIXES) /
    /// [`DOC_COMMENT_SKIP_KINDS`](Self::DOC_COMMENT_SKIP_KINDS) to scan
    /// preceding siblings. Returns `None` when `DOC_COMMENT_KIND` is `None`.
    fn extract_doc_range(node: TsNode<'_>) -> Option<Range<usize>> {
        let kind = Self::DOC_COMMENT_KIND?;
        extract_preceding_doc_range(node, kind, Self::DOC_COMMENT_PREFIXES, Self::DOC_COMMENT_SKIP_KINDS)
    }

    /// Extract the byte range of the file-level doc comment (e.g. `//!` in
    /// Rust, module docstring in Python). Returns `None` when the file has
    /// no module-level documentation.
    fn extract_file_doc_range(_root: TsNode<'_>) -> Option<Range<usize>> { None }

    /// Strip language-specific doc comment markers from raw text.
    fn strip_doc_comment(raw: &str) -> String { raw.to_owned() }

    /// Wrap plain text in language-specific doc comment markers.
    fn wrap_doc_comment(plain: &str, _indent: &str) -> String { plain.to_owned() }

    /// Wrap plain text in file-level doc comment markers (e.g. `//!` in Rust).
    /// Default delegates to [`wrap_doc_comment`](Self::wrap_doc_comment).
    fn wrap_file_doc_comment(plain: &str, indent: &str) -> String { Self::wrap_doc_comment(plain, indent) }

    /// Extract the first non-empty sentence from a raw doc comment.
    fn clean_doc_comment(raw: &str) -> Option<String> {
        let stripped = Self::strip_doc_comment(raw);
        stripped
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(String::from)
    }

    /// Extract visibility information from a symbol node.
    fn extract_visibility(_node: TsNode<'_>) -> Option<String> { None }

    /// Extract decorator/attribute byte range for a symbol node.
    fn extract_decorator_range(_node: TsNode<'_>) -> Option<Range<usize>> { None }
}

/// Extract doc comment ranges by scanning preceding siblings.
///
/// Shared helper for languages where doc comments appear before the symbol
/// definition (Rust `///`, TypeScript `/** */`).
///
/// `match_prefixes` is a list of accepted prefixes — the comment text must
/// start with at least one of them to be collected.
pub fn extract_preceding_doc_range(
    node: TsNode<'_>,
    match_kind: &str,
    match_prefixes: &[&str],
    skip_kinds: &[&str],
) -> Option<Range<usize>> {
    merge_preceding_sibling_ranges(node, |sib| {
        if sib.kind() == match_kind && match_prefixes.iter().any(|p| sib.text().starts_with(p)) {
            Some(true)
        } else if skip_kinds.contains(&sib.kind()) {
            Some(false)
        } else {
            None
        }
    })
}

/// Extract decorator/attribute ranges by scanning preceding siblings.
///
/// Shared helper for languages where decorators appear before the symbol
/// definition (Rust `#[...]`, TypeScript `@decorator`).
pub fn extract_preceding_decorator_range(node: TsNode<'_>, decorator_kind: &str) -> Option<Range<usize>> {
    merge_preceding_sibling_ranges(node, |sib| (sib.kind() == decorator_kind).then_some(true))
}

/// Extract visibility by finding a child node of a specific kind.
///
/// Shared helper for languages where visibility is a child node
/// (Rust `visibility_modifier`).
pub fn extract_child_visibility(node: TsNode<'_>, kind: &str) -> Option<String> {
    node.children().find(|c| c.kind() == kind).map(|c| c.text().to_owned())
}

/// Strip line-comment prefixes from raw doc comment text.
///
/// Shared helper for languages using line-based doc comments (Rust `///`/`//!`,
/// TypeScript `///`/`//`). Tries each prefix in order; first match wins.
pub fn strip_line_comment_prefixes(raw: &str, prefixes: &[&str]) -> String {
    raw.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            for prefix in prefixes {
                if let Some(rest) = trimmed.strip_prefix(prefix) {
                    return rest.strip_prefix(' ').unwrap_or(rest).to_owned();
                }
            }
            line.to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Wrap plain text with line-comment doc markers.
///
/// Shared helper for languages using line-prefixed doc comments.
/// `bare_prefix` is used for empty lines (e.g. `"///"`).
/// `space_prefix` is used for content lines (e.g. `"/// "`).
pub fn wrap_line_doc_comment(plain: &str, indent: &str, bare_prefix: &str, space_prefix: &str) -> String {
    let mut result = String::new();
    for (i, line) in plain.lines().enumerate() {
        if i > 0 {
            result.push('\n');
            result.push_str(indent);
        }
        if line.is_empty() {
            result.push_str(bare_prefix);
        } else {
            result.push_str(space_prefix);
            result.push_str(line);
        }
    }
    result
}

/// Public API for decomposing source files.
pub trait Decomposer: Send + Sync {
    fn decompose(&self, source: &str, max_depth: usize) -> (DecomposedFile, Option<tree_sitter::Tree>);
    fn validate(&self, source: &str) -> Result<()>;
    fn language_name(&self) -> &'static str;
    fn file_extension(&self) -> &'static str;
    fn strip_doc_comment(&self, raw: &str) -> String;
    fn wrap_doc_comment(&self, plain: &str, indent: &str) -> String;
    fn wrap_file_doc_comment(&self, plain: &str, indent: &str) -> String;
    fn clean_doc_comment(&self, raw: &str) -> Option<String>;
    fn map_to_fs(&self, fragments: &mut [Fragment]);
    fn resolve_conflicts(&self, conflicts: &[ConflictSet]) -> Vec<Resolution>;
    fn splice_mode(&self) -> SpliceMode;
}

/// Generic decomposer that derives [`Decomposer`] from [`LanguageSpec`].
///
/// Languages using the standard extraction loop AND languages using custom
/// extraction both use this single decomposer — `extract_custom()` controls
/// which path is taken.
pub struct CodeDecomposer<L: LanguageSpec> {
    parser: TreeSitterParser,
    ext: &'static str,
    _lang: PhantomData<L>,
}

/// Constructor for `CodeDecomposer`, initializing the tree-sitter parser for the language.
impl<L: LanguageSpec> CodeDecomposer<L> {
    /// Creates a new decomposer for the given file extension.
    #[must_use]
    pub fn new(ext: &'static str) -> Self {
        let grammar = L::grammar(ext);
        Self {
            parser: TreeSitterParser::new(&grammar),
            ext,
            _lang: PhantomData,
        }
    }
}

/// [`Decomposer`] implementation that delegates to [`LanguageSpec`] methods.
impl<L: LanguageSpec> Decomposer for CodeDecomposer<L> {
    /// Decomposes source code into fragments using tree-sitter.
    fn decompose(&self, source: &str, max_depth: usize) -> (DecomposedFile, Option<tree_sitter::Tree>) {
        self.parser.decompose(
            source,
            max_depth,
            L::IMPORT_KINDS,
            |root, depth| {
                // Custom extraction bypasses the standard loop.
                if let Some(fragments) = L::extract_custom(root, depth) {
                    return fragments;
                }
                extract_fragments::<L>(root, depth, None)
            },
            L::extract_file_doc_range,
        )
    }

    /// Validates source code syntax via tree-sitter.
    fn validate(&self, source: &str) -> Result<()> { self.parser.validate(source, L::NAME) }

    /// Returns the language name.
    fn language_name(&self) -> &'static str { L::NAME }

    /// Returns the file extension this decomposer handles.
    fn file_extension(&self) -> &'static str { self.ext }

    /// Strips doc comment prefixes from raw text.
    fn strip_doc_comment(&self, raw: &str) -> String { L::strip_doc_comment(raw) }

    /// Wraps plain text in language-specific doc comment syntax.
    fn wrap_doc_comment(&self, plain: &str, indent: &str) -> String { L::wrap_doc_comment(plain, indent) }

    /// Wraps plain text in file-level doc comment syntax.
    fn wrap_file_doc_comment(&self, plain: &str, indent: &str) -> String { L::wrap_file_doc_comment(plain, indent) }

    /// Cleans a doc comment for display.
    fn clean_doc_comment(&self, raw: &str) -> Option<String> { L::clean_doc_comment(raw) }

    /// Applies filesystem name mapping to fragments.
    fn map_to_fs(&self, fragments: &mut [Fragment]) { apply_fs_mapping(fragments, L::NAMING_STRATEGY); }

    /// Resolves naming conflicts between fragments.
    fn resolve_conflicts(&self, conflicts: &[ConflictSet]) -> Vec<Resolution> {
        resolve_conflicts(conflicts, L::CONFLICT_STRATEGY)
    }

    /// Returns the splice mode for this language.
    fn splice_mode(&self) -> SpliceMode { L::SPLICE_MODE }
}
