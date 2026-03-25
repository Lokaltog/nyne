/// Code generation: transforms validated route AST into token streams.
mod codegen;
/// Parsing: converts `routes!` token input into a typed AST.
pub mod parse;

use parse::{ParsedPattern, RouteEntry, RoutesInput, SegmentRoute};
use proc_macro2::TokenStream;
use syn::Result;

/// Validate and expand parsed route input into a `RouteTree` token stream.
pub fn expand(input: &RoutesInput) -> Result<TokenStream> {
    validate_entries(&input.entries)?;
    codegen::generate(input)
}

/// Validate a list of route entries for duplicate segments and ambiguous patterns at each level.
fn validate_entries(entries: &[RouteEntry]) -> Result<()> {
    let mut exact_names: Vec<(String, proc_macro2::Span)> = Vec::new();
    let mut capture_count: usize = 0;
    let mut rest_count: usize = 0;
    let mut glob_count: usize = 0;

    for entry in entries {
        let RouteEntry::Segment(seg) = entry else {
            continue;
        };
        validate_segment(
            seg,
            &mut exact_names,
            &mut capture_count,
            &mut rest_count,
            &mut glob_count,
        )?;
    }
    Ok(())
}

/// Validate a single segment route: reject duplicate exact names, multiple captures/globs, and invalid lookup patterns.
fn validate_segment(
    seg: &SegmentRoute,
    exact_names: &mut Vec<(String, proc_macro2::Span)>,
    capture_count: &mut usize,
    rest_count: &mut usize,
    glob_count: &mut usize,
) -> Result<()> {
    match &seg.parsed_pattern {
        ParsedPattern::Exact(name) => {
            if exact_names.iter().any(|(n, _)| n == name) {
                return Err(syn::Error::new(
                    seg.span,
                    format!("duplicate exact segment \"{name}\" at this level"),
                ));
            }
            exact_names.push((name.clone(), seg.span));
        }
        ParsedPattern::Capture { .. } => {
            *capture_count += 1;
            if *capture_count > 1 {
                return Err(syn::Error::new(
                    seg.span,
                    "multiple captures at the same level are ambiguous",
                ));
            }
        }
        ParsedPattern::RestCapture { .. } => {
            *rest_count += 1;
            if *rest_count > 1 {
                return Err(syn::Error::new(
                    seg.span,
                    "multiple rest captures at the same level are ambiguous",
                ));
            }
        }
        ParsedPattern::Glob => {
            *glob_count += 1;
            if *glob_count > 1 {
                return Err(syn::Error::new(
                    seg.span,
                    "multiple globs at the same level are ambiguous",
                ));
            }
        }
    }

    // Lookup patterns are validated during parsing (must be Capture with prefix/suffix).

    // Recursively validate children — delegate to `validate_entries` which
    // sets up fresh accumulators and filters for segments.
    validate_entries(&seg.sub_routes)?;

    Ok(())
}
