//! Fragment types: the decomposed pieces of a source file.
//!
//! A [`Fragment`] represents either a code symbol or a document section.
//! [`DecomposedFile`] holds the results of decomposing a complete file.

use std::fmt::{self, Display, Formatter};
use std::iter;
use std::ops::Range;

use strum::IntoStaticStr;

/// Kind of a top-level source-code symbol.
///
/// Cross-language superset: not every variant applies to every language.
/// Language decomposers map tree-sitter node kinds to these via
/// `LanguageSpec::map_symbol_kind` / the `symbol_map!` macro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IntoStaticStr)]
#[strum(serialize_all = "lowercase")]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    Const,
    Static,
    TypeAlias,
    Impl,
    Macro,
    Class,
    Interface,
    Module,
    Variable,
    Decorator,
}

/// Display and classification methods for `SymbolKind`.
impl SymbolKind {
    /// Filesystem directory name for this symbol kind (lowercased variant form).
    ///
    /// Derived from the `IntoStaticStr` impl on [`SymbolKind`] which uses
    /// `#[strum(serialize_all = "lowercase")]`.
    #[must_use]
    pub fn directory_name(self) -> &'static str { self.into() }

    /// Whether this symbol kind is a scope that can contain child items.
    ///
    /// Scope symbols (impl blocks, traits, enums, etc.) can accept
    /// `edit/append` even when they have no children yet.
    pub const fn is_scope(self) -> bool {
        matches!(
            self,
            Self::Impl | Self::Trait | Self::Enum | Self::Struct | Self::Class | Self::Module | Self::Interface
        )
    }
}

/// The kind of a fragment extracted from a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FragmentKind {
    /// A source-code symbol (function, struct, impl block, etc.).
    Symbol(SymbolKind),
    /// A doc comment — file-level (`//!`) or symbol-level (`///`).
    /// Appears as a child of its owning symbol, or at the top level for
    /// file-level documentation.
    Docstring,
    /// A contiguous block of import/use declarations.
    Imports,
    /// Decorators or attributes preceding a symbol definition.
    Decorator,
    /// A document section identified by a heading level.
    Section { level: u8 },
    /// A fenced code block inside a document section.
    CodeBlock { lang: Option<String> },
    /// Non-symbol content at the top of a file: frontmatter, bare config
    /// keys, extends/import directives, etc. Collected as a single unit
    /// rather than decomposed into individual fragments.
    Preamble,
}

/// Query methods for `FragmentKind`.
impl FragmentKind {
    /// Structural fragments are metadata (docstrings, imports, decorators)
    /// rather than navigable symbols. They should not receive `fs_name` or
    /// appear as `@/` directories in the VFS.
    pub const fn is_structural(&self) -> bool { matches!(self, Self::Docstring | Self::Imports | Self::Decorator) }

    /// Concise display string for OVERVIEW.md symbol tables.
    ///
    /// Differs from [`Display`] only for sections (`h2` vs `Section(h2)`).
    /// All other variants delegate to the `Display` impl.
    pub fn short_display(&self) -> String {
        match self {
            Self::Section { level } => format!("h{level}"),
            _ => self.to_string(),
        }
    }
}

/// Display `SymbolKind` as its variant name (e.g. `Function`, `TypeAlias`).
///
/// The `IntoStaticStr` derive uses `serialize_all = "lowercase"` for the
/// filesystem directory name, so `Display` is implemented via `Debug` to
/// preserve the CamelCase form consumers (templates, logs) expect.
impl Display for SymbolKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result { write!(f, "{self:?}") }
}

impl Display for FragmentKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Symbol(kind) => write!(f, "{kind}"),
            Self::Docstring => write!(f, "Docstring"),
            Self::Imports => write!(f, "Imports"),
            Self::Decorator => write!(f, "Decorator"),
            Self::Section { level } => write!(f, "Section(h{level})"),
            Self::CodeBlock { lang: Some(lang) } => write!(f, "CodeBlock({lang})"),
            Self::CodeBlock { lang: None } => write!(f, "CodeBlock"),
            Self::Preamble => write!(f, "Preamble"),
        }
    }
}

/// Document-specific metadata for non-code fragments (sections, code blocks).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FragmentMetadata {
    /// A document section (e.g. markdown heading). `index` is the sequential
    /// position among siblings for filesystem name disambiguation.
    Document { index: usize },
    /// A fenced code block inside a document section.
    CodeBlock { index: usize },
}

/// A single decomposed piece of a file -- a code symbol, docstring, import
/// block, decorator, or document section.
///
/// Fragments form a tree: a function fragment may have docstring and
/// decorator children, and an impl block may have method children.
/// Each fragment carries its source byte range so that the VFS can
/// serve content directly from the original source and splice writes
/// back into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fragment {
    pub name: String,
    pub kind: FragmentKind,
    /// Byte range of this fragment's own content. For symbols, this is the
    /// tree-sitter node range (excluding doc comments and decorators, which
    /// are separate child fragments). Use [`full_span()`](Self::full_span)
    /// for the bounding box including children.
    pub byte_range: Range<usize>,
    pub signature: Option<String>,
    /// Visibility qualifier (`pub`, `pub(crate)`, etc.). Only meaningful for
    /// code symbols; `None` for all other fragment kinds.
    pub visibility: Option<String>,
    /// Document-specific metadata (section/code-block index). `None` for
    /// code symbols and structural fragments.
    pub metadata: Option<FragmentMetadata>,
    /// Byte offset of the name token in the source text.
    ///
    /// Used by LSP operations (rename, references, call hierarchy) to send
    /// the correct position. For sections and synthetic names (e.g. impl
    /// blocks), this equals `byte_range.start`.
    pub name_byte_offset: usize,
    /// Nested fragments: docstrings, decorators, methods, nested functions.
    pub children: Vec<Self>,
    /// Name of the parent fragment, if this fragment is nested inside another.
    pub parent_name: Option<String>,
    /// Filesystem-safe name for this fragment. Populated by FS mapping.
    ///
    /// `None` means this fragment is hidden from the filesystem (not yet mapped
    /// or explicitly suppressed).
    pub fs_name: Option<String>,
}

/// Construction and span computation methods for fragments.
impl Fragment {
    /// Construct a structural fragment (docstring, decorator, imports) from a
    /// byte range. These carry no signature, no visibility, and no children.
    pub fn structural(
        name: impl Into<String>,
        kind: FragmentKind,
        byte_range: Range<usize>,
        parent_name: Option<String>,
    ) -> Self {
        let name_byte_offset = byte_range.start;
        Self {
            name: name.into(),
            kind,
            byte_range,
            signature: None,
            visibility: None,
            metadata: None,
            name_byte_offset,
            children: vec![],
            parent_name,
            fs_name: None,
        }
    }

    /// Construct a docstring child fragment from an optional byte range.
    ///
    /// Returns `Some` if `doc_range` is present, `None` otherwise.
    /// This is the single construction point for docstring fragments —
    /// language extractors should use this instead of calling `structural` directly.
    pub fn docstring_child(doc_range: Option<Range<usize>>, parent: Option<String>) -> Option<Self> {
        let range = doc_range?;
        Some(Self::structural("docstring", FragmentKind::Docstring, range, parent))
    }

    /// Bounding box covering this fragment and all its children (docstrings,
    /// decorators, nested symbols). Derived from `byte_range` — never stale.
    pub fn full_span(&self) -> Range<usize> {
        let start = iter::once(self.byte_range.start)
            .chain(self.children.iter().map(|c| c.byte_range.start))
            .min()
            .unwrap_or(self.byte_range.start);
        let end = iter::once(self.byte_range.end)
            .chain(self.children.iter().map(|c| c.byte_range.end))
            .max()
            .unwrap_or(self.byte_range.end);
        start..end
    }

    /// Line range (0-based, exclusive end) covering this fragment and all
    /// its children. Requires a pre-built rope for byte→line conversion.
    pub fn line_range(&self, rope: &crop::Rope) -> Range<usize> {
        let span = self.full_span();
        rope.line_of_byte(span.start)..rope.line_of_byte(span.end) + 1
    }

    /// Find a child fragment by kind.
    pub fn child_of_kind(&self, kind: &FragmentKind) -> Option<&Self> { self.children.iter().find(|c| c.kind == *kind) }

    /// Find a child fragment by filesystem name.
    pub fn child_by_fs_name(&self, name: &str) -> Option<&Self> {
        self.children.iter().find(|c| c.fs_name.as_deref() == Some(name))
    }
}

/// A decomposed source file: a flat/tree of fragments in source order.
pub type DecomposedFile = Vec<Fragment>;

/// Find the first fragment of a given kind in a fragment slice.
///
/// Linear scan; typically used on small child lists to locate the
/// imports or docstring fragment for a symbol.
pub fn find_fragment_of_kind<'a>(fragments: &'a [Fragment], kind: &FragmentKind) -> Option<&'a Fragment> {
    fragments.iter().find(|f| f.kind == *kind)
}

/// A syntax error detected by tree-sitter in the parse tree.
///
/// Collected from ERROR and MISSING nodes after parsing. Used by the
/// validation step in splice operations to reject edits that would
/// introduce syntax errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// 0-based start line.
    pub start_line: usize,
    /// 0-based start column.
    pub start_col: usize,
    /// 0-based end line.
    pub end_line: usize,
    /// 0-based end column.
    pub end_col: usize,
    /// The erroneous source text (truncated to 120 chars).
    pub text: String,
}

/// A group of fragments that share the same `fs_name` and need disambiguation.
///
/// Passed to [`Decomposer::resolve_conflicts`] which returns [`Resolution`]s
/// with updated names (typically `~Kind` suffixed, e.g. `Foo~Struct` vs `Foo~Impl`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictSet {
    /// The colliding filesystem name.
    pub name: String,
    /// The fragments involved in the collision.
    pub entries: Vec<ConflictEntry>,
}

/// One fragment participating in a name conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictEntry {
    /// Index into the flat fragment list for back-patching.
    pub index: usize,
    /// The fragment's kind (used for `~Kind` disambiguation).
    pub fragment_kind: FragmentKind,
}

/// The resolved filesystem name for one conflicting fragment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    /// Index matching [`ConflictEntry::index`].
    pub index: usize,
    /// New `fs_name` value. `None` hides the fragment.
    pub fs_name: Option<String>,
}
