use proc_macro2::Span;
use syn::parse::{Parse, ParseStream};
use syn::token::{Brace, Paren};
use syn::{Expr, Ident, LitStr, Result, Token, Type, braced};

/// Top-level `routes!(Self, { ... })` input.
pub struct RoutesInput {
    pub provider_ty: Type,
    pub entries: Vec<RouteEntry>,
}

/// A single entry inside a `{ ... }` block.
pub enum RouteEntry {
    /// `"segment" => handler { ... }` or `"segment" { ... }`
    Segment(SegmentRoute),
    /// `lookup(handler)` or `lookup "pattern" => handler`
    Lookup(LookupEntry),
    /// `file("name", readable_expr)` with optional modifiers
    File(FileEntry),
    /// `children(handler)` — root-level children handler
    Children(Ident),
}

/// A segment route with optional handler and children.
pub struct SegmentRoute {
    pub pattern: LitStr,
    pub children_handler: Option<Ident>,
    pub lookups: Vec<LookupEntry>,
    pub files: Vec<FileEntry>,
    pub sub_routes: Vec<RouteEntry>,
    pub span: Span,
    /// Suppress auto-emission of a directory entry in parent readdir.
    pub no_emit: bool,
}

/// A lookup entry — either catch-all or pattern-based.
pub enum LookupEntry {
    /// `lookup(handler)`
    CatchAll { handler: Ident },
    /// `lookup "{name}.ext" => handler`
    Pattern { pattern: LitStr, handler: Ident },
}

/// `file("name", readable_expr)` with optional `.no_cache()`, `.hidden()`, `.sliceable()`.
///
/// The content expression is a `Readable` impl — the macro wraps it in
/// `VirtualNode::file(name, readable)` and chains any modifiers.
pub struct FileEntry {
    pub name: LitStr,
    pub content: Expr,
    pub modifiers: Vec<FileModifier>,
}

/// A chained modifier on a `file()` declaration.
pub enum FileModifier {
    NoCache,
    Hidden,
    Sliceable,
}

/// Parsed segment pattern from a string literal.
pub enum ParsedPattern {
    /// `"literal"` — exact match
    Exact(String),
    /// `"**"` — glob
    Glob,
    /// `"{name}"`, `"{name}@"`, or `"BLAME.md:{spec}"` — single capture
    /// with optional prefix and/or suffix around the `{name}`.
    Capture {
        name: String,
        prefix: Option<String>,
        suffix: Option<String>,
    },
    /// `"{..name}"` or `"{..name}@"` — rest capture
    RestCapture { name: String, suffix: Option<String> },
}

/// Parse `routes!(ProviderType, { ... })` input into a [`RoutesInput`] AST.
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

/// Parse the contents of a `{ ... }` block into a list of route entries, separated by optional commas.
fn parse_entries(input: ParseStream<'_>) -> Result<Vec<RouteEntry>> {
    let mut entries = Vec::new();
    while !input.is_empty() {
        entries.push(parse_entry(input)?);
        // Optional trailing comma
        let _ = input.parse::<Token![,]>();
    }
    Ok(entries)
}

/// Parse a single route entry by lookahead: `lookup`, `file`, `children`, or a segment string literal.
fn parse_entry(input: ParseStream<'_>) -> Result<RouteEntry> {
    // Lookahead: `lookup`, `file`, `children`, or string literal
    if input.peek(Ident) {
        let ident: Ident = input.fork().parse()?;
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
            let handler: Ident = content.parse()?;
            return Ok(RouteEntry::Children(handler));
        }
    }

    // Must be a segment route
    Ok(RouteEntry::Segment(parse_segment_route(input)?))
}

/// Parse a segment route: optional `no_emit`, a pattern string, optional `=> handler`, and optional `{ ... }` children.
fn parse_segment_route(input: ParseStream<'_>) -> Result<SegmentRoute> {
    // Optional `no_emit` keyword — suppresses directory auto-emission.
    let no_emit = if input.peek(Ident) && input.peek2(LitStr) {
        let ident: Ident = input.fork().parse()?;
        if ident == "no_emit" {
            input.parse::<Ident>()?; // consume from real stream
            true
        } else {
            false
        }
    } else {
        false
    };

    let pattern: LitStr = input.parse()?;
    let span = pattern.span();
    let mut children_handler = None;
    let mut lookups = Vec::new();
    let mut files = Vec::new();
    let mut sub_routes = Vec::new();

    // Optional `=> handler`
    if input.peek(Token![=>]) {
        input.parse::<Token![=>]>()?;

        // Check for `lookup(handler)` after =>
        if input.peek(Ident) {
            let ident: Ident = input.fork().parse()?;
            if ident == "lookup" {
                // `"**" => lookup(handler)` form
                let lookup = parse_lookup(input)?;
                lookups.push(lookup);
                return Ok(SegmentRoute {
                    pattern,
                    children_handler,
                    lookups,
                    files,
                    sub_routes,
                    span,
                    no_emit,
                });
            }
        }
        children_handler = Some(input.parse::<Ident>()?);
    }

    // Optional `{ ... }` block with children
    if input.peek(Brace) {
        let content;
        braced!(content in input);
        while !content.is_empty() {
            let entry = parse_entry(&content)?;
            match entry {
                RouteEntry::Lookup(l) => lookups.push(l),
                RouteEntry::File(f) => files.push(f),
                RouteEntry::Segment(_) => sub_routes.push(entry),
                RouteEntry::Children(_) => {
                    return Err(syn::Error::new(
                        pattern.span(),
                        "children() inside a segment block is redundant — use `=> handler` on the segment instead",
                    ));
                }
            }
            let _ = content.parse::<Token![,]>();
        }
    }

    Ok(SegmentRoute {
        pattern,
        children_handler,
        lookups,
        files,
        sub_routes,
        span,
        no_emit,
    })
}

/// Parse a lookup entry: either `lookup(handler)` (catch-all) or `lookup "pattern" => handler` (pattern-based).
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
        let pattern: LitStr = input.parse()?;
        input.parse::<Token![=>]>()?;
        let handler: Ident = input.parse()?;
        Ok(LookupEntry::Pattern { pattern, handler })
    }
}

/// Parse a `file("name", expr)` declaration with optional chained modifiers.
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

/// Parse chained `.no_cache()`, `.hidden()`, and `.sliceable()` modifiers after a `file(...)` declaration.
fn parse_file_modifiers(input: ParseStream<'_>) -> Result<Vec<FileModifier>> {
    let mut modifiers = Vec::new();
    while input.peek(Token![.]) {
        input.parse::<Token![.]>()?;
        let ident: Ident = input.parse()?;
        let modifier = match ident.to_string().as_str() {
            "no_cache" => FileModifier::NoCache,
            "hidden" => FileModifier::Hidden,
            "sliceable" => FileModifier::Sliceable,
            other =>
                return Err(syn::Error::new(
                    ident.span(),
                    format!("unknown file modifier `{other}`"),
                )),
        };
        // Consume optional empty parens: `.no_cache()`
        if input.peek(Paren) {
            let _content;
            syn::parenthesized!(_content in input);
        }
        modifiers.push(modifier);
    }
    Ok(modifiers)
}

/// Validate brace usage in a pattern string, ensuring at most one `{name}` capture group.
///
/// Returns `Ok(None)` for literal patterns (no braces), `Ok(Some((open, close)))` for a single
/// valid capture pair, or `Err` for any malformed brace usage.
fn validate_braces(s: &str, lit: &LitStr) -> syn::Result<Option<(usize, usize)>> {
    let open_count = s.chars().filter(|&c| c == '{').count();
    let close_count = s.chars().filter(|&c| c == '}').count();

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

    // At this point we have at most one of each
    let open = s.find('{');
    let close = s.find('}');

    match (open, close) {
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
        (None, None) => Ok(None), // already handled above, but for completeness
    }
}
/// Parse a segment pattern string into its typed representation.
///
/// Supports prefix and suffix around captures: `"BLAME.md:{spec}"`,
/// `"{name}@"`, `"pre-{x}-suf"`.
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

/// Validate that a capture name is a valid Rust identifier (ASCII alphanumeric or underscore, starting with a letter or underscore).
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
