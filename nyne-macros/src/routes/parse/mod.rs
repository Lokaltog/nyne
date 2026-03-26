//! Parsing phase of the `routes!` macro.
//!
//! Converts the raw token stream from a `routes!(ProviderType, { ... })` invocation into
//! a typed AST ([`RoutesInput`] containing [`RouteEntry`] trees). This module defines both
//! the AST types and the syn-based parsing functions that populate them.
//!
//! Pattern strings (e.g., `"segment"`, `"{capture}@"`, `"**"`) are parsed into
//! [`ParsedPattern`] variants during this phase, so downstream validation and codegen
//! can work with structured data rather than raw strings.

use proc_macro2::Span;
use syn::parse::{Parse, ParseStream};
use syn::token::{Brace, Paren};
use syn::{Expr, Ident, LitStr, Result, Token, Type, braced};

/// Top-level AST node for a `routes!(ProviderType, { ... })` invocation.
///
/// The `provider_ty` is the type that all handler method calls will be dispatched on
/// (typically `Self` when used inside a `Provider` impl). The `entries` are the
/// top-level route declarations inside the braced block.
pub struct RoutesInput {
    pub provider_ty: Type,
    pub entries: Vec<RouteEntry>,
}

/// A single entry inside a `{ ... }` route block.
///
/// Route blocks can contain four kinds of entries, each contributing different
/// behavior to the virtual directory tree. The variant determines how the entry
/// is validated and what code is generated for it.
pub enum RouteEntry {
    /// `"segment" => handler { ... }` or `"segment" { ... }` — a directory node
    /// that defines a path segment with optional handler and nested children.
    Segment(SegmentRoute),
    /// `lookup(handler)` or `lookup "pattern" => handler` — dynamic name resolution
    /// for names that don't match any static segment.
    Lookup(LookupEntry),
    /// `file("name", readable_expr)` with optional modifiers — a static virtual file
    /// whose content is provided by a `Readable` expression.
    File(FileEntry),
    /// `children(handler)` — root-level children handler that delegates directory
    /// listing to a provider method. Only valid at the top level of the route block;
    /// inside segments, use `=> handler` on the segment itself instead.
    Children(Ident),
}

/// A segment route with optional handler, lookups, files, and sub-routes.
///
/// Represents a single path segment in the virtual directory tree (e.g., `"symbols"`
/// or `"{name}@"`). A segment can optionally:
/// - Have a `children_handler` that provides dynamic directory entries via `=> handler`.
/// - Contain nested `lookups` for dynamic name resolution within the segment.
/// - Declare static `files` that appear as children.
/// - Nest further `sub_routes` as child segments.
///
/// The `span` is preserved from the pattern string literal for error reporting.
pub struct SegmentRoute {
    pub parsed_pattern: ParsedPattern,
    pub children_handler: Option<Ident>,
    pub lookups: Vec<LookupEntry>,
    pub files: Vec<FileEntry>,
    pub sub_routes: Vec<Self>,
    pub span: Span,
    /// Suppress auto-emission of a directory entry in parent readdir.
    ///
    /// When set, the dispatch layer will not automatically include this segment
    /// in the parent's directory listing. Useful for "hidden" structural segments
    /// that should only be reachable by explicit path traversal.
    pub no_emit: bool,
}

/// A lookup entry for dynamic name resolution within a segment or at root level.
///
/// Lookups handle names that don't match any static segment. Two forms exist:
/// - **Catch-all**: `lookup(handler)` — receives every unmatched name.
/// - **Pattern-based**: `lookup "{name}.ext" => handler` — matches names with a
///   specific prefix/suffix, extracting the captured portion as a route parameter.
pub enum LookupEntry {
    /// `lookup(handler)` — receives all unmatched names verbatim.
    CatchAll { handler: Ident },
    /// `lookup "{name}.ext" => handler` — matches names with prefix/suffix stripping,
    /// injecting the captured value into route params.
    Pattern { pattern: ParsedPattern, handler: Ident },
}

/// A static file declaration: `file("name", readable_expr)` with optional modifiers.
///
/// The content expression must implement `Readable` — the macro wraps it in
/// `VirtualNode::file(name, readable)` and chains any modifiers. Modifiers control
/// caching, visibility, and sliceability of the generated virtual file node.
pub struct FileEntry {
    pub name: LitStr,
    pub content: Expr,
    pub modifiers: Vec<FileModifier>,
}

/// A chained modifier on a `file()` declaration.
///
/// Modifiers are parsed from `.method()` chains after the `file(...)` call and
/// control runtime behavior of the generated virtual file node.
pub enum FileModifier {
    /// `.no_cache()` — disables dispatch-layer caching, forcing fresh content on every read.
    NoCache,
    /// `.hidden()` — excludes the file from parent directory listings while remaining
    /// accessible by direct path.
    Hidden,
    /// `.sliceable()` — enables line-range slicing via `file.ext@/lines:M-N` syntax.
    Sliceable,
}

/// Parsed segment pattern from a string literal.
///
/// Pattern strings in the `routes!` DSL are parsed into this enum during the parsing
/// phase. Each variant represents a different matching strategy used by the runtime
/// route tree to resolve path segments.
///
/// Examples of pattern strings and their parsed forms:
/// - `"symbols"` -> `Exact("symbols")`
/// - `"**"` -> `Glob`
/// - `"{name}"` -> `Capture { name: "name", prefix: None, suffix: None }`
/// - `"{name}@"` -> `Capture { name: "name", prefix: None, suffix: Some("@") }`
/// - `"BLAME.md:{spec}"` -> `Capture { name: "spec", prefix: Some("BLAME.md:"), suffix: None }`
/// - `"{..path}"` -> `RestCapture { name: "path", suffix: None }`
#[cfg_attr(test, derive(Debug, PartialEq))]
#[derive(Clone)]
pub enum ParsedPattern {
    /// `"literal"` — exact string match against a single path segment.
    Exact(String),
    /// `"**"` — glob that matches any remaining path segments.
    Glob,
    /// `"{name}"`, `"{name}@"`, or `"BLAME.md:{spec}"` — single-segment capture
    /// with optional prefix and/or suffix around the `{name}` placeholder. At runtime,
    /// the prefix/suffix are stripped and the remaining text is captured as a route parameter.
    Capture {
        name: String,
        prefix: Option<String>,
        suffix: Option<String>,
    },
    /// `"{..name}"` or `"{..name}@"` — rest capture that consumes remaining path segments.
    ///
    /// Unlike `Capture`, rest captures cannot have a prefix (enforced during parsing)
    /// because they match from the current position to the end of the path.
    RestCapture { name: String, suffix: Option<String> },
}

/// Parse `routes!(ProviderType, { ... })` input into a [`RoutesInput`] AST.
///
/// Implements syn's `Parse` trait so the macro can use `parse_macro_input!` for
/// automatic error reporting. The expected token structure is:
/// 1. A type expression (the provider type, usually `Self`).
/// 2. A comma separator.
/// 3. A braced block containing route entries.
impl Parse for RoutesInput {
    /// Parse the provider type, comma, and braced route entries from the token stream.
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let provider_ty: Type = input.parse()?;
        input.parse::<Token![,]>()?;
        let content;
        braced!(content in input);
        let entries = parse_entries(&content)?;
        Ok(Self { provider_ty, entries })
    }
}

/// Parse the contents of a `{ ... }` block into a list of route entries.
///
/// Entries are separated by optional commas (trailing commas are allowed and
/// encouraged for consistency, but not required). Parsing continues until the
/// input stream is exhausted.
fn parse_entries(input: ParseStream<'_>) -> Result<Vec<RouteEntry>> {
    let mut entries = Vec::new();
    while !input.is_empty() {
        entries.push(parse_entry(input)?);
        // Optional trailing comma
        let _ = input.parse::<Token![,]>();
    }
    Ok(entries)
}

/// Parse a single route entry by lookahead on the first token.
///
/// Uses cursor-based lookahead (not `peek`) to distinguish keyword-prefixed entries
/// (`lookup`, `file`, `children`) from segment routes (string literals). This avoids
/// consuming tokens before knowing which entry type we're parsing.
fn parse_entry(input: ParseStream<'_>) -> Result<RouteEntry> {
    // Lookahead: `lookup`, `file`, `children`, `no_emit`, or string literal
    if let Some((ident, _)) = input.cursor().ident() {
        if ident == "lookup" {
            return Ok(RouteEntry::Lookup(parse_lookup(input)?));
        }
        if ident == "file" {
            return Ok(RouteEntry::File(parse_file(input)?));
        }
        if ident == "children" {
            let _: Ident = input.parse()?; // consume "children"
            let content;
            syn::parenthesized!(content in input);
            return Ok(RouteEntry::Children(content.parse()?));
        }
        // `no_emit` is a valid segment route prefix — fall through to parse_segment_route.
        if ident != "no_emit" {
            return Err(syn::Error::new(
                ident.span(),
                format!("unknown route entry kind `{ident}`"),
            ));
        }
    }

    // Must be a segment route (string literal or `no_emit "pattern" ...`)
    Ok(RouteEntry::Segment(parse_segment_route(input)?))
}

/// Parse entries inside a segment's `{ ... }` block, distributing them into the route.
///
/// Unlike top-level entries, `children()` is not allowed inside a segment block —
/// the segment's own `=> handler` syntax serves the same purpose. Lookups, files,
/// and sub-segments are accumulated into the corresponding fields on the parent
/// [`SegmentRoute`].
fn parse_segment_children(content: ParseStream<'_>, route: &mut SegmentRoute) -> Result<()> {
    while !content.is_empty() {
        match parse_entry(content)? {
            RouteEntry::Lookup(l) => route.lookups.push(l),
            RouteEntry::File(f) => route.files.push(f),
            RouteEntry::Segment(s) => route.sub_routes.push(s),
            RouteEntry::Children(_) => {
                return Err(syn::Error::new(
                    route.span,
                    "children() inside a segment block is redundant — use `=> handler` on the segment instead",
                ));
            }
        }
        let _ = content.parse::<Token![,]>();
    }
    Ok(())
}

/// Parse a segment route from a pattern string with optional handler and children.
///
/// The full grammar for a segment is:
/// ```text
/// [no_emit] "pattern" [=> handler | => lookup(handler)] [{ children }]
/// ```
///
/// - `no_emit` is an optional keyword that suppresses directory auto-emission.
/// - The pattern string is parsed into a [`ParsedPattern`] via [`parse_pattern`].
/// - `=> handler` attaches a children handler method; `=> lookup(handler)` is
///   shorthand for a catch-all lookup on a glob segment.
/// - The optional `{ ... }` block contains nested entries (sub-segments, lookups, files).
fn parse_segment_route(input: ParseStream<'_>) -> Result<SegmentRoute> {
    // Optional `no_emit` keyword — suppresses directory auto-emission.
    let no_emit = if matches!(input.cursor().ident(), Some((ident, _)) if ident == "no_emit") && input.peek2(LitStr) {
        let _ = input.parse::<Ident>(); // consume "no_emit" (cursor confirmed it exists)
        true
    } else {
        false
    };

    let pattern: LitStr = input.parse()?;
    let mut route = SegmentRoute {
        parsed_pattern: parse_pattern(&pattern)?,
        children_handler: None,
        lookups: Vec::new(),
        files: Vec::new(),
        sub_routes: Vec::new(),
        span: pattern.span(),
        no_emit,
    };

    // Optional `=> handler`
    if input.peek(Token![=>]) {
        input.parse::<Token![=>]>()?;

        // `"**" => lookup(handler)` form
        if matches!(input.cursor().ident(), Some((ident, _)) if ident == "lookup") {
            route.lookups.push(parse_lookup(input)?);
            return Ok(route);
        }
        route.children_handler = Some(input.parse::<Ident>()?);
    }

    // Optional `{ ... }` block with children
    if input.peek(Brace) {
        let content;
        braced!(content in input);
        parse_segment_children(&content, &mut route)?;
    }

    Ok(route)
}

/// Parse a lookup entry in one of two forms.
///
/// - **Catch-all**: `lookup(handler)` — parenthesized handler identifier receives
///   every unmatched name.
/// - **Pattern-based**: `lookup "pattern" => handler` — the pattern must be a
///   `Capture` variant with at least a prefix or suffix so there is something to
///   strip. A bare `{name}` capture without prefix/suffix is rejected because it
///   would match everything (use catch-all form instead).
fn parse_lookup(input: ParseStream<'_>) -> Result<LookupEntry> {
    let _: Ident = input.parse()?; // consume "lookup"

    if input.peek(Paren) {
        // `lookup(handler)`
        let content;
        syn::parenthesized!(content in input);
        let handler: Ident = content.parse()?;
        Ok(LookupEntry::CatchAll { handler })
    } else {
        // `lookup "pattern" => handler`
        let lit: LitStr = input.parse()?;
        let pattern = parse_pattern(&lit)?;
        let ParsedPattern::Capture {
            ref prefix, ref suffix, ..
        } = pattern
        else {
            return Err(syn::Error::new(
                lit.span(),
                "lookup pattern must be a capture (e.g., \"{name}.ext\" or \"PREFIX:{name}\")",
            ));
        };
        if prefix.is_none() && suffix.is_none() {
            return Err(syn::Error::new(
                lit.span(),
                "lookup pattern capture must have a prefix or suffix (e.g., \"{}.ext\" or \"BLAME.md:{}\")",
            ));
        }
        input.parse::<Token![=>]>()?;
        let handler: Ident = input.parse()?;
        Ok(LookupEntry::Pattern { pattern, handler })
    }
}

/// Parse a `file("name", expr)` declaration with optional chained modifiers.
///
/// The parenthesized arguments are a string literal name and an expression that
/// implements `Readable`. After the closing paren, any `.modifier()` chains are
/// consumed by [`parse_file_modifiers`].
fn parse_file(input: ParseStream<'_>) -> Result<FileEntry> {
    let _: Ident = input.parse()?; // consume "file"
    let content;
    syn::parenthesized!(content in input);
    let name: LitStr = content.parse()?;
    content.parse::<Token![,]>()?;
    let expr: Expr = content.parse()?;
    let modifiers = parse_file_modifiers(input)?;
    Ok(FileEntry {
        name,
        content: expr,
        modifiers,
    })
}

/// Resolve a modifier identifier string to its [`FileModifier`] enum variant.
///
/// Returns a compile error for unrecognized modifier names, listing the identifier
/// that was not understood. This is the single point where valid modifier names are
/// defined — adding a new modifier requires only adding a match arm here and a
/// variant to [`FileModifier`].
fn resolve_file_modifier(ident: &Ident) -> Result<FileModifier> {
    match ident.to_string().as_str() {
        "no_cache" => Ok(FileModifier::NoCache),
        "hidden" => Ok(FileModifier::Hidden),
        "sliceable" => Ok(FileModifier::Sliceable),
        other => Err(syn::Error::new(
            ident.span(),
            format!("unknown file modifier `{other}`"),
        )),
    }
}

/// Parse chained `.modifier()` calls after a `file(...)` declaration.
///
/// Consumes zero or more `.ident()` sequences from the input stream. Each identifier
/// is resolved via [`resolve_file_modifier`]. The empty parentheses after each modifier
/// are optional — both `.no_cache()` and `.no_cache` are accepted for ergonomics.
fn parse_file_modifiers(input: ParseStream<'_>) -> Result<Vec<FileModifier>> {
    let mut modifiers = Vec::new();
    while input.peek(Token![.]) {
        input.parse::<Token![.]>()?;
        modifiers.push(resolve_file_modifier(&input.parse::<Ident>()?)?);
        // Consume optional empty parens: `.no_cache()`
        if input.peek(Paren) {
            let _content;
            syn::parenthesized!(_content in input);
        }
    }
    Ok(modifiers)
}

/// Validate brace usage in a pattern string, ensuring well-formed capture syntax.
///
/// Enforces the constraint that pattern strings may contain at most one `{name}`
/// capture group. Returns the byte positions of the opening and closing braces
/// if a valid capture is found, `None` for literal patterns (no braces), or an
/// error for any malformed usage:
/// - Multiple `{` or `}` characters.
/// - Mismatched or reversed braces (`}` before `{`).
/// - Empty capture name (`{}`).
/// - Unclosed `{` or unopened `}`.
fn validate_braces(s: &str, lit: &LitStr) -> syn::Result<Option<(usize, usize)>> {
    // Single-pass scan collecting counts and first positions of `{` and `}`.
    let (open_count, close_count, open_pos, close_pos) =
        s.char_indices()
            .fold((0u32, 0u32, None, None), |(oc, cc, op, cp), (i, ch)| match ch {
                '{' => (oc + 1, cc, op.or(Some(i)), cp),
                '}' => (oc, cc + 1, op, cp.or(Some(i))),
                _ => (oc, cc, op, cp),
            });

    if open_count == 0 && close_count == 0 {
        return Ok(None);
    }

    if open_count > 1 {
        return Err(syn::Error::new(
            lit.span(),
            "pattern contains multiple `{` — only one capture group `{name}` is supported",
        ));
    }
    if close_count > 1 {
        return Err(syn::Error::new(
            lit.span(),
            "pattern contains multiple `}` — only one capture group `{name}` is supported",
        ));
    }

    match (open_pos, close_pos) {
        (Some(o), Some(c)) => {
            if c < o {
                return Err(syn::Error::new(
                    lit.span(),
                    "closing `}` appears before opening `{` in pattern",
                ));
            }
            if c == o + 1 {
                return Err(syn::Error::new(
                    lit.span(),
                    "empty capture name in pattern — expected `{name}`",
                ));
            }
            Ok(Some((o, c)))
        }
        (Some(_), None) => Err(syn::Error::new(
            lit.span(),
            "unclosed `{` in pattern — expected `{name}`",
        )),
        (None, Some(_)) => Err(syn::Error::new(
            lit.span(),
            "unopened `}` in pattern — expected `{name}`",
        )),
        (None, None) => unreachable!("both counts are non-zero but neither char was found"),
    }
}

/// Parse a segment pattern string literal into its typed [`ParsedPattern`] representation.
///
/// This is the core pattern parser that converts DSL string literals into structured
/// data. The parsing logic:
/// 1. `"**"` is recognized as a glob.
/// 2. Strings without braces are exact literals.
/// 3. Strings with `{name}` become captures; text before/after the braces becomes
///    the prefix/suffix.
/// 4. `{..name}` denotes a rest capture (consumes remaining path segments).
///    Rest captures cannot have a prefix — only an optional suffix.
///
/// Brace validation is delegated to [`validate_braces`], and capture names are
/// validated by [`validate_capture_name`] to ensure they are valid Rust identifiers
/// (since they become route parameter keys at runtime).
pub fn parse_pattern(lit: &LitStr) -> Result<ParsedPattern> {
    let s = lit.value();

    if s == "**" {
        return Ok(ParsedPattern::Glob);
    }

    let Some((open, close)) = validate_braces(&s, lit)? else {
        return Ok(ParsedPattern::Exact(s));
    };

    let inner = &s[open + 1..close];

    let (is_rest, name) = if let Some(rest_name) = inner.strip_prefix("..") {
        (true, rest_name)
    } else {
        (false, inner)
    };

    if name.is_empty() {
        return Err(syn::Error::new(lit.span(), "empty capture name"));
    }
    validate_capture_name(name, lit)?;

    let prefix = (open > 0).then(|| s[..open].to_owned());
    let suffix = (close + 1 < s.len()).then(|| s[close + 1..].to_owned());

    if is_rest {
        if prefix.is_some() {
            return Err(syn::Error::new(
                lit.span(),
                "rest captures (`{..name}`) cannot have a prefix",
            ));
        }
        Ok(ParsedPattern::RestCapture {
            name: name.to_owned(),
            suffix,
        })
    } else {
        Ok(ParsedPattern::Capture {
            name: name.to_owned(),
            prefix,
            suffix,
        })
    }
}

/// Validate that a capture name is a valid Rust identifier.
///
/// Capture names become route parameter keys at runtime, so they must follow Rust
/// identifier rules: start with an ASCII letter or underscore, followed by ASCII
/// alphanumeric characters or underscores. Non-ASCII identifiers are deliberately
/// not supported to keep parameter lookup simple and predictable.
fn validate_capture_name(name: &str, lit: &LitStr) -> Result<()> {
    if !name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') {
        return Err(syn::Error::new(
            lit.span(),
            format!("capture name '{name}' is not a valid identifier"),
        ));
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(syn::Error::new(
            lit.span(),
            format!("capture name '{name}' contains invalid characters"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
