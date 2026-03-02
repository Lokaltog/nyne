mod codegen;
pub mod parse;

use parse::{ParsedPattern, RouteEntry, RoutesInput, SegmentRoute, parse_pattern};
use proc_macro2::TokenStream;
use syn::Result;

pub fn expand(input: &RoutesInput) -> Result<TokenStream> {
    validate_entries(&input.entries)?;
    codegen::generate(input)
}

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

fn validate_segment(
    seg: &SegmentRoute,
    exact_names: &mut Vec<(String, proc_macro2::Span)>,
    capture_count: &mut usize,
    rest_count: &mut usize,
    glob_count: &mut usize,
) -> Result<()> {
    let pattern = parse_pattern(&seg.pattern)?;

    match &pattern {
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

    // Validate lookup patterns: must be a capture with prefix and/or suffix.
    for lookup in &seg.lookups {
        if let parse::LookupEntry::Pattern { pattern, .. } = lookup {
            let parsed = parse_pattern(pattern)?;
            let is_bounded = matches!(
                &parsed,
                ParsedPattern::Capture { prefix: Some(_), .. } | ParsedPattern::Capture { suffix: Some(_), .. }
            );
            if !is_bounded {
                return Err(syn::Error::new(
                    pattern.span(),
                    "lookup pattern must be a capture with prefix or suffix \
                     (e.g., \"{ref}.diff\" or \"BLAME.md:{spec}\")",
                ));
            }
        }
    }

    // Recursively validate children
    let child_segments: Vec<&SegmentRoute> = seg
        .sub_routes
        .iter()
        .filter_map(|e| if let RouteEntry::Segment(s) = e { Some(s) } else { None })
        .collect();

    if !child_segments.is_empty() {
        let mut child_exacts = Vec::new();
        let mut child_captures = 0;
        let mut child_rests = 0;
        let mut child_globs = 0;

        for child in child_segments {
            validate_segment(
                child,
                &mut child_exacts,
                &mut child_captures,
                &mut child_rests,
                &mut child_globs,
            )?;
        }
    }

    Ok(())
}
