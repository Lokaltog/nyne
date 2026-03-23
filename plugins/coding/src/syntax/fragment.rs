//! Fragment types: the decomposed pieces of a source file.
//!
//! A [`Fragment`] represents either a code symbol or a document section.
//! [`DecomposedFile`] holds the results of decomposing a complete file.

use std::fmt::{self, Display, Formatter};
use std::ops::Range;

use nyne::types::line_of_byte;
use strum::{Display as StrumDisplay, EnumString};

/// Kind of a top-level source-code symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, StrumDisplay, EnumString)]
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

impl SymbolKind {
    /// Filesystem directory name for this symbol kind (lowercased display form).
    pub fn directory_name(self) -> String { self.to_string().to_lowercase() }

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
    /// A document section identified by a heading level.
    Section { level: u8 },
    /// A fenced code block inside a document section.
    CodeBlock { lang: Option<String> },
    /// Non-symbol content at the top of a file: frontmatter, bare config
    /// keys, extends/import directives, etc. Collected as a single unit
    /// rather than decomposed into individual fragments.
    Preamble,
}

impl Display for FragmentKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Symbol(kind) => write!(f, "{kind}"),
            Self::Section { level } => write!(f, "Section(h{level})"),
            Self::CodeBlock { lang: Some(lang) } => write!(f, "CodeBlock({lang})"),
            Self::CodeBlock { lang: None } => write!(f, "CodeBlock"),
            Self::Preamble => write!(f, "Preamble"),
        }
    }
}

/// Language-specific metadata attached to a [`Fragment`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FragmentMetadata {
    /// Metadata for source-code fragments.
    Code {
        visibility: Option<String>,
        doc_comment_range: Option<Range<usize>>,
        /// Byte range of decorators/attributes preceding the symbol definition.
        decorator_range: Option<Range<usize>>,
    },
    /// Metadata for document fragments (e.g. markdown sections).
    Document { index: usize },
    /// Metadata for fenced code blocks inside document sections.
    CodeBlock { index: usize },
}

/// A single decomposed piece of a file — either a code symbol or a document
/// section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fragment {
    pub name: String,
    pub kind: FragmentKind,
    /// Byte range of the tree-sitter node (or wrapper node for decorated
    /// definitions). Does NOT include preceding doc comments or decorators
    /// that live outside the node — use `full_span` for the complete range.
    pub byte_range: Range<usize>,
    /// Byte range covering the complete symbol definition: decorators + doc
    /// comment + signature + body. Computed at decomposition time by
    /// [`LanguageSpec::full_symbol_range`](super::spec::LanguageSpec::full_symbol_range) (bounding box of all components).
    ///
    /// For document fragments, equals `byte_range`.
    pub full_span: Range<usize>,
    pub line_range: Range<usize>,
    pub signature: Option<String>,
    pub metadata: FragmentMetadata,
    /// Byte offset of the name token in the source text.
    ///
    /// Used by LSP operations (rename, references, call hierarchy) to send
    /// the correct position. For sections and synthetic names (e.g. impl
    /// blocks), this equals `byte_range.start`.
    pub name_byte_offset: usize,
    /// Nested fragments (e.g. methods inside an impl block, nested functions).
    pub children: Vec<Self>,
    /// Name of the parent fragment, if this fragment is nested inside another.
    pub parent_name: Option<String>,
    /// Filesystem-safe name for this fragment. Populated by FS mapping.
    ///
    /// `None` means this fragment is hidden from the filesystem (not yet mapped
    /// or explicitly suppressed).
    pub fs_name: Option<String>,
}

impl Fragment {
    /// Construct a fragment, computing `line_range` from `full_span` + `source`.
    ///
    /// This is the single source of truth for `Fragment` assembly. All
    /// construction sites (code symbols, document sections, code blocks)
    /// must use this constructor instead of struct literals.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source: &str,
        name: String,
        kind: FragmentKind,
        byte_range: Range<usize>,
        full_span: Range<usize>,
        signature: Option<String>,
        metadata: FragmentMetadata,
        name_byte_offset: usize,
        children: Vec<Self>,
        parent_name: Option<String>,
    ) -> Self {
        let line_range = line_of_byte(source, full_span.start)..line_of_byte(source, full_span.end) + 1;
        Self {
            name,
            kind,
            byte_range,
            full_span,
            line_range,
            signature,
            metadata,
            name_byte_offset,
            children,
            parent_name,
            fs_name: None,
        }
    }
}

/// The result of decomposing a file into its constituent fragments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecomposedFile {
    pub fragments: Vec<Fragment>,
    pub imports: Option<ImportSpan>,
    /// First line of the file-level doc comment (e.g. `//!` in Rust, module
    /// docstring in Python). `None` when the file has no module-level doc.
    pub file_doc: Option<String>,
}

impl DecomposedFile {
    /// Return an empty decomposition result (no fragments, no imports).
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            fragments: Vec::new(),
            imports: None,
            file_doc: None,
        }
    }
}

/// A contiguous range of import declarations extracted from source code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportSpan {
    pub byte_range: Range<usize>,
    pub line_range: Range<usize>,
    pub content: String,
}

/// Default maximum nesting depth for recursive fragment extraction.
pub const DEFAULT_MAX_DEPTH: usize = 5;

/// A syntax error detected by tree-sitter in the parse tree.
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
    /// The fragment's logical name.
    pub fragment_name: String,
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
