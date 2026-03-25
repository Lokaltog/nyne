/// Code generation: transforms validated route AST into token streams.
mod codegen;
/// Parsing: converts `routes!` token input into a typed AST.
pub mod parse;

use std::collections::HashMap;

use parse::{ParsedPattern, RouteEntry, RoutesInput, SegmentRoute};
use proc_macro2::{Span, TokenStream};
use syn::Result;

/// Validate and expand parsed route input into a `RouteTree` token stream.
pub fn expand(input: &RoutesInput) -> Result<TokenStream> {
    validate_entries(&input.entries)?;
    codegen::generate(input)
}

/// Validate a list of route entries for duplicate segments and ambiguous patterns at each level.
fn validate_entries(entries: &[RouteEntry]) -> Result<()> {
    validate_segments(
        &entries
            .iter()
            .filter_map(|e| if let RouteEntry::Segment(s) = e { Some(s) } else { None })
            .collect::<Vec<_>>(),
    )
}

/// Validate a list of segment routes for duplicates and ambiguous patterns at each level.
fn validate_segments(segments: &[&SegmentRoute]) -> Result<()> {
    let mut exact_names: HashMap<String, Span> = HashMap::new();
    let mut capture_span: Option<Span> = None;
    let mut rest_span: Option<Span> = None;
    let mut glob_span: Option<Span> = None;

    for seg in segments {
        validate_segment(seg, &mut exact_names, &mut capture_span, &mut rest_span, &mut glob_span)?;
    }
    Ok(())
}

/// Validate a single segment route: reject duplicate exact names, multiple captures/globs, and invalid lookup patterns.
fn validate_segment(
    seg: &SegmentRoute,
    exact_names: &mut HashMap<String, Span>,
    capture_span: &mut Option<Span>,
    rest_span: &mut Option<Span>,
    glob_span: &mut Option<Span>,
) -> Result<()> {
    match &seg.parsed_pattern {
        ParsedPattern::Exact(name) => {
            if let Some(&first) = exact_names.get(name.as_str()) {
                let mut err = syn::Error::new(seg.span, format!("duplicate exact segment \"{name}\" at this level"));
                err.combine(syn::Error::new(first, "first defined here"));
                return Err(err);
            }
            exact_names.insert(name.clone(), seg.span);
        }
        ParsedPattern::Capture { .. } => {
            reject_duplicate(capture_span, seg.span, "captures")?;
        }
        ParsedPattern::RestCapture { .. } => {
            reject_duplicate(rest_span, seg.span, "rest captures")?;
        }
        ParsedPattern::Glob => {
            reject_duplicate(glob_span, seg.span, "globs")?;
        }
    }

    // Lookup patterns are validated during parsing (must be Capture with prefix/suffix).

    // Recursively validate children with fresh accumulators.
    validate_segments(&seg.sub_routes.iter().collect::<Vec<_>>())?;

    Ok(())
}

/// Reject a second occurrence of a pattern kind, pointing at both locations.
fn reject_duplicate(first: &mut Option<Span>, current: Span, kind: &str) -> Result<()> {
    if let Some(prev) = *first {
        let mut err = syn::Error::new(current, format!("multiple {kind} at the same level are ambiguous"));
        err.combine(syn::Error::new(prev, "first defined here"));
        return Err(err);
    }
    *first = Some(current);
    Ok(())
}
