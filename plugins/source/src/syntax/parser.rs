//! Tree-sitter parsing utilities and code fragment construction.
//!
//! Provides [`TsNode`] (an ergonomic wrapper around `tree_sitter::Node`),
//! shared helpers for collecting byte ranges (doc comments, imports,
//! decorators), and [`TreeSitterParser`] which encapsulates the thread-safe
//! parse-and-decompose pipeline. Language decomposers depend on this module
//! for all tree-sitter interaction.

use std::ops::Range;
use std::str::from_utf8;

use color_eyre::eyre::{Result, eyre};
use parking_lot::Mutex;

use super::fragment::{DecomposedFile, Fragment, FragmentKind, ParseError, SymbolKind};

/// Ergonomic wrapper around a [`tree_sitter::Node`] paired with its source
/// bytes, eliminating the `(node, &[u8])` parameter pairs that dominate the
/// raw tree-sitter API.
#[derive(Clone, Copy)]
pub struct TsNode<'a> {
    node: tree_sitter::Node<'a>,
    source: &'a [u8],
}

/// Methods for navigating and extracting data from tree-sitter nodes.
impl<'a> TsNode<'a> {
    /// Create a new `TsNode` wrapping a raw tree-sitter node and its source bytes.
    pub const fn new(node: tree_sitter::Node<'a>, source: &'a [u8]) -> Self { Self { node, source } }

    /// The tree-sitter node kind string (e.g. `"function_item"`, `"class_definition"`).
    pub fn kind(&self) -> &'static str { self.node.kind() }

    /// The UTF-8 text of this node, or empty string if invalid.
    pub fn text(&self) -> &'a str { self.node.utf8_text(self.source).unwrap_or("") }

    /// Byte range `start..end` of this node in the source.
    pub fn byte_range(&self) -> Range<usize> { self.node.byte_range() }

    /// Start byte offset of this node in the source.
    pub fn start_byte(&self) -> usize { self.node.start_byte() }

    /// End byte offset of this node in the source.
    pub fn end_byte(&self) -> usize { self.node.end_byte() }

    /// Access a named field child (e.g. `"name"`, `"body"`, `"type"`).
    pub fn field(&self, name: &str) -> Option<Self> {
        self.node.child_by_field_name(name).map(|n| Self::new(n, self.source))
    }

    /// Text of a named field child, or `None` if the field is absent.
    pub fn field_text(&self, name: &str) -> Option<&'a str> { self.field(name).map(|n| n.text()) }

    /// Text content up to (but not including) the first occurrence of `ch`.
    /// Falls back to the first line if `ch` is not found.
    pub fn text_up_to(&self, ch: char) -> String {
        let full = self.text();
        if let Some(pos) = full.find(ch) {
            let sig = full[..pos].trim();
            if sig.is_empty() {
                return self.first_line().to_owned();
            }
            return sig.to_owned();
        }
        self.first_line().to_owned()
    }

    /// First line of this node's text, trimmed.
    pub fn first_line(&self) -> &'a str { self.text().lines().next().unwrap_or("").trim() }

    /// Build a type signature string like `"pub struct Foo"` from keyword and visibility.
    pub fn type_signature(&self, keyword: &str, visibility: Option<&str>) -> String {
        let name = self.field_text("name").unwrap_or("?");
        match visibility {
            Some(v) => format!("{v} {keyword} {name}"),
            None => format!("{keyword} {name}"),
        }
    }

    /// Byte offset of the `name` field child's start, if present.
    pub fn name_start_byte(&self) -> Option<usize> { self.node.child_by_field_name("name").map(|n| n.start_byte()) }

    /// The `body` field child, if present.
    pub fn body(&self) -> Option<Self> { self.field("body") }

    /// Parent node, if any.
    pub fn parent(&self) -> Option<Self> { self.node.parent().map(|n| Self::new(n, self.source)) }

    /// Previous sibling node, if any.
    pub fn prev_sibling(&self) -> Option<Self> { self.node.prev_sibling().map(|n| Self::new(n, self.source)) }

    /// All direct children as a `Vec`.
    ///
    /// Allocation is required because the tree cursor is borrowed for the
    /// lifetime of the tree-sitter child iterator.
    pub fn children(&self) -> Vec<Self> {
        let source = self.source;
        let mut cursor = self.node.walk();
        self.node
            .children(&mut cursor)
            .map(move |n| Self::new(n, source))
            .collect()
    }

    /// Access the underlying `tree_sitter::Node` for operations not yet
    /// wrapped by [`TsNode`].
    ///
    /// Prefer dedicated [`TsNode`] methods when they exist — this escape
    /// hatch exists for downstream analysis plugins that need cursor walks,
    /// descendant counts, or other raw tree-sitter APIs.
    pub const fn raw(&self) -> tree_sitter::Node<'a> { self.node }

    /// Source bytes this node was created with.
    pub const fn source(&self) -> &'a [u8] { self.source }

    /// Source bytes interpreted as UTF-8 (infallible for valid source).
    pub fn source_str(&self) -> &'a str { from_utf8(self.source).unwrap_or("") }
}

/// Trim trailing `\n` bytes from a byte range.
///
/// Tree-sitter node ranges for line-based constructs (Rust `line_comment`,
/// attributes) include the trailing newline. Raw `node.end_byte()` points
/// past the `\n`, not at the last content byte. Range-merging code that
/// produces content for splice operations must trim this newline so the
/// separator between the collected content and the following symbol is
/// preserved. See `syntax/CLAUDE.md` for the full convention.
pub fn trim_trailing_newlines(source: &[u8], range: Range<usize>) -> Range<usize> {
    let Range { start, mut end } = range;
    while end > start && source.get(end - 1) == Some(&b'\n') {
        end -= 1;
    }
    start..end
}

/// Depth-first search for the first descendant (inclusive of `node`) matching `kind`.
pub fn find_first_descendant<'a>(node: TsNode<'a>, kind: &str) -> Option<TsNode<'a>> {
    if node.kind() == kind {
        return Some(node);
    }
    for child in node.children() {
        if let Some(found) = find_first_descendant(child, kind) {
            return Some(found);
        }
    }
    None
}

/// Recursively collect all descendants (inclusive of `node`) of the given kind,
/// mapping each matched node via `parse` and appending the result to `out`.
pub fn collect_descendants<T>(node: TsNode<'_>, kind: &str, parse: &impl Fn(TsNode<'_>) -> T, out: &mut Vec<T>) {
    if node.kind() == kind {
        out.push(parse(node));
    }
    for child in node.children() {
        collect_descendants(child, kind, parse, out);
    }
}

/// Merge byte ranges of preceding siblings matched by `collect_fn` into a single span.
///
/// Walks backwards from `node`, trimming trailing newlines from the result.
/// Used to collect doc-comment and attribute blocks that precede a symbol
/// definition. The `collect_fn` returns `Some(true)` to include a sibling,
/// `Some(false)` to skip it (e.g. blank lines), or `None` to stop walking.
///
/// Trailing newlines are stripped via [`trim_trailing_newlines`] — keeping
/// them would eat the separator between the collected content and the
/// following symbol during splice operations.
pub fn merge_preceding_sibling_ranges(
    node: TsNode<'_>,
    mut collect_fn: impl FnMut(TsNode<'_>) -> Option<bool>,
) -> Option<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut sibling = node.prev_sibling();

    while let Some(sib) = sibling {
        match collect_fn(sib) {
            Some(true) => {
                ranges.push(sib.byte_range());
                sibling = sib.prev_sibling();
            }
            Some(false) => {
                sibling = sib.prev_sibling();
            }
            None => break,
        }
    }

    // Ranges were collected bottom-up; last element is the topmost.
    let first = ranges.first()?;
    let end = first.end;
    let start = ranges.last().map_or(first.start, |r| r.start);

    Some(trim_trailing_newlines(node.source(), start..end))
}

/// Collect the byte range spanning all import declarations at the root level.
///
/// Returns `None` when the root has no children matching `import_kinds`.
/// Trailing newlines are trimmed via [`trim_trailing_newlines`].
pub fn collect_import_range(root: TsNode<'_>, import_kinds: &[&str]) -> Option<Range<usize>> {
    let (start, end) = root
        .children()
        .into_iter()
        .filter(|child| import_kinds.contains(&child.kind()))
        .fold(None, |acc, node| {
            let s = node.start_byte();
            let e = node.end_byte();
            Some(match acc {
                None => (s, e),
                Some((first, _)) => (first, e),
            })
        })?;

    Some(trim_trailing_newlines(root.source(), start..end))
}

/// Walk a tree-sitter tree and collect all ERROR and MISSING nodes.
///
/// Entry point for syntax validation. The returned [`ParseError`]s are
/// used by splice validation to reject edits that introduce syntax errors
/// and by DIAGNOSTICS.md rendering to show parse problems.
pub fn collect_parse_errors(tree: &tree_sitter::Tree, source: &[u8]) -> Vec<ParseError> {
    let mut errors = Vec::new();
    let mut cursor = tree.walk();
    collect_errors_recursive(&mut cursor, source, &mut errors);
    errors
}

/// Recursively walk the tree-sitter tree collecting ERROR and MISSING nodes.
///
/// Uses a `TreeCursor` for stack-efficient traversal. Error text is
/// truncated to 120 chars to keep diagnostic output readable.
fn collect_errors_recursive(cursor: &mut tree_sitter::TreeCursor<'_>, source: &[u8], errors: &mut Vec<ParseError>) {
    let node = cursor.node();
    if node.is_error() || node.is_missing() {
        let start = node.start_position();
        let end = node.end_position();
        let raw = node.utf8_text(source).unwrap_or("");
        let text = if raw.len() > 120 {
            format!("{}...", &raw[..raw.floor_char_boundary(120)])
        } else {
            raw.to_owned()
        };
        errors.push(ParseError {
            start_line: start.row,
            start_col: start.column,
            end_line: end.row,
            end_col: end.column,
            text,
        });
        return;
    }
    if cursor.goto_first_child() {
        loop {
            collect_errors_recursive(cursor, source, errors);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

/// Specification for constructing a code [`Fragment`] via
/// [`build_code_fragment`].
///
/// Collects all the language-specific data extracted from a tree-sitter node
/// into a single struct, so `build_code_fragment` can handle the common
/// `Fragment` assembly logic.
pub struct CodeFragmentSpec {
    pub name: String,
    pub kind: SymbolKind,
    pub signature: String,
    pub name_byte_offset: usize,
    pub visibility: Option<String>,
    pub children: Vec<Fragment>,
}

/// Build a [`Fragment`] from a tree-sitter node and a [`CodeFragmentSpec`].
///
/// Thin adapter that bridges the language-specific data collected in
/// [`CodeFragmentSpec`] with the generic [`Fragment::new`] constructor.
/// The byte range is taken from the tree-sitter `span_node`.
pub fn build_code_fragment(span_node: TsNode<'_>, spec: CodeFragmentSpec, parent_name: Option<&str>) -> Fragment {
    Fragment::new(
        spec.name,
        FragmentKind::Symbol(spec.kind),
        span_node.byte_range(),
        Some(spec.signature),
        spec.visibility,
        None,
        spec.name_byte_offset,
        spec.children,
        parent_name.map(String::from),
    )
}
/// Build a [`Fragment`] for nodes with the common pattern: first-line signature,
/// docstring child from doc range, no extra children, and no visibility.
///
/// This covers the majority of simple language symbols (Fennel forms, TOML
/// tables, Nix bindings without nested attribute sets). Languages with
/// additional children or custom signatures should use [`build_code_fragment`]
/// directly.
pub fn build_simple_fragment(
    node: TsNode<'_>,
    name: String,
    kind: SymbolKind,
    doc_range: Option<Range<usize>>,
    parent_name: Option<&str>,
) -> Fragment {
    let signature = node.first_line().to_owned();
    let parent = Some(name.clone());
    let children: Vec<Fragment> = Fragment::docstring_child(doc_range, parent).into_iter().collect();

    build_code_fragment(
        node,
        CodeFragmentSpec {
            name,
            kind,
            signature,
            name_byte_offset: node.start_byte(),
            visibility: None,
            children,
        },
        parent_name,
    )
}

/// Shared tree-sitter parser wrapper used by all language decomposers.
///
/// Encapsulates the `Mutex<Parser>` pattern and provides common operations
/// (parse, validate, error collection) so individual decomposers don't
/// duplicate this boilerplate.
pub struct TreeSitterParser {
    parser: Mutex<tree_sitter::Parser>,
}

/// Core parsing and decomposition methods for `TreeSitterParser`.
impl TreeSitterParser {
    /// Create a new tree-sitter parser for the given language grammar.
    ///
    /// The parser is wrapped in a `Mutex` so the same `TreeSitterParser`
    /// instance can be used from multiple threads (tree-sitter parsers
    /// themselves are not `Sync`).
    ///
    /// # Panics
    ///
    /// Panics if the language cannot be set (indicates a build/version
    /// mismatch between the linked grammar and the tree-sitter runtime).
    #[allow(clippy::expect_used)] // language is a linked grammar, failure = build mismatch
    pub fn new(language: &tree_sitter::Language) -> Self {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(language)
            .expect("failed to set tree-sitter language");
        Self {
            parser: Mutex::new(parser),
        }
    }

    /// Parse `source` with tree-sitter and return the tree.
    pub fn parse(&self, source: &str) -> Option<tree_sitter::Tree> {
        let mut parser = self.parser.lock();
        parser.parse(source, None)
    }

    /// Validate that `source` parses without syntax errors.
    pub fn validate(&self, source: &str, lang_name: &str) -> Result<()> {
        let tree = self
            .parse(source)
            .ok_or_else(|| eyre!("tree-sitter failed to parse {lang_name} source"))?;
        if tree.root_node().has_error() {
            let errors = collect_parse_errors(&tree, source.as_bytes());
            let detail: Vec<String> = errors
                .iter()
                .take(3)
                .map(|e| {
                    format!(
                        "  L{}:{}-L{}:{}: {:?}",
                        e.start_line + 1,
                        e.start_col,
                        e.end_line + 1,
                        e.end_col,
                        e.text,
                    )
                })
                .collect();
            let mut suffix = String::new();
            if !detail.is_empty() {
                suffix.push('\n');
                suffix.push_str(&detail.join("\n"));
            }
            return Err(eyre!(
                "{lang_name} source contains syntax errors ({} error(s), source_len={}){suffix}",
                errors.len(),
                source.len(),
            ));
        }
        Ok(())
    }

    /// Run the standard decomposition pipeline: parse → extract fragments →
    /// collect imports → extract file doc. All results are unified into
    /// `DecomposedFile.fragments`, sorted by byte position.
    ///
    /// Language-specific behavior is injected via closures.
    pub fn decompose(
        &self,
        source: &str,
        max_depth: usize,
        import_kinds: &[&str],
        extract_fragments: impl FnOnce(TsNode<'_>, usize) -> Vec<Fragment>,
        extract_file_doc_range: impl FnOnce(TsNode<'_>) -> Option<Range<usize>>,
    ) -> (DecomposedFile, Option<tree_sitter::Tree>) {
        let Some(tree) = self.parse(source) else {
            return (Vec::new(), None);
        };
        let src = source.as_bytes();
        let root = TsNode::new(tree.root_node(), src);
        let mut fragments = extract_fragments(root, max_depth);

        if let Some(range) = collect_import_range(root, import_kinds) {
            fragments.push(Fragment::structural("imports", FragmentKind::Imports, range, None));
        }

        if let Some(range) = extract_file_doc_range(root) {
            fragments.push(Fragment::structural("file_doc", FragmentKind::Docstring, range, None));
        }

        // Natural source order: file_doc → imports → preamble → symbols.
        fragments.sort_by_key(|f| f.byte_range.start);

        (fragments, Some(tree))
    }
}
