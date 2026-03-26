//! Route tree macro implementation: parsing, validation, and code generation.
//!
//! This module orchestrates the three-phase pipeline for the [`routes!`](crate::routes) macro.
//! The public entry point is [`expand`], which validates a parsed AST and delegates to the
//! [`codegen`] module for token generation. Validation happens here rather than during parsing
//! so that parse errors are reported first (syntax before semantics).

/// Code generation: transforms validated route AST into token streams.
mod codegen;
/// Parsing: converts `routes!` token input into a typed AST.
pub mod parse;

use std::collections::HashMap;

use parse::{ParsedPattern, RouteEntry, RoutesInput, SegmentRoute};
use proc_macro2::{Span, TokenStream};
use syn::Result;

/// Validate and expand parsed route input into a `RouteTree` token stream.
///
/// This is the main entry point called by the [`routes!`](crate::routes) proc macro after
/// parsing. Validation runs first to catch structural errors (duplicate segments, ambiguous
/// patterns) before code generation begins — this ensures users see the most actionable
/// error message rather than confusing generated-code failures.
pub fn expand(input: &RoutesInput) -> Result<TokenStream> {
    validate_entries(&input.entries)?;
    codegen::generate(input)
}

/// Validate a list of route entries for structural correctness.
///
/// Filters out non-segment entries (lookups, files, children) since only segments
/// can conflict with each other at the same nesting level, then delegates to
/// [`validate_segments`] for the actual duplicate/ambiguity checks.
fn validate_entries(entries: &[RouteEntry]) -> Result<()> {
    validate_segments(
        &entries
            .iter()
            .filter_map(|e| if let RouteEntry::Segment(s) = e { Some(s) } else { None })
            .collect::<Vec<_>>(),
    )
}

/// Per-level accumulator for segment validation.
///
/// Tracks the spans of pattern kinds seen so far at a single nesting level,
/// enabling duplicate/ambiguity detection across sibling segments.
struct ValidationState {
    exact_names: HashMap<String, Span>,
    capture_span: Option<Span>,
    rest_span: Option<Span>,
    glob_span: Option<Span>,
}
/// Validate a list of segment routes for duplicates and ambiguous patterns.
///
/// Initializes per-kind tracking state and validates each segment against it.
/// This enforces the rule that at any given nesting level there can be at most
/// one capture, one rest capture, and one glob — but any number of distinct
/// exact segments.
fn validate_segments(segments: &[&SegmentRoute]) -> Result<()> {
    let mut state = ValidationState {
        exact_names: HashMap::new(),
        capture_span: None,
        rest_span: None,
        glob_span: None,
    };

    for seg in segments {
        validate_segment(seg, &mut state)?;
    }
    Ok(())
}

/// Validate a single segment route against the accumulated state for its nesting level.
///
/// Enforces three invariants per nesting level:
/// 1. No two exact segments share the same name (would be ambiguous during lookup).
/// 2. At most one capture pattern (multiple `{name}` patterns cannot be distinguished).
/// 3. At most one rest capture and one glob (same ambiguity reason).
///
/// After checking the current segment, recursively validates its children with fresh
/// accumulators — each nesting level has independent uniqueness constraints.
///
/// Lookup patterns are not validated here; they are checked during parsing to ensure
/// they are `Capture` variants with at least a prefix or suffix.
fn validate_segment(seg: &SegmentRoute, state: &mut ValidationState) -> Result<()> {
    match &seg.parsed_pattern {
        ParsedPattern::Exact(name) => {
            if let Some(&first) = state.exact_names.get(name.as_str()) {
                let mut err = syn::Error::new(seg.span, format!("duplicate exact segment \"{name}\" at this level"));
                err.combine(syn::Error::new(first, "first defined here"));
                return Err(err);
            }
            state.exact_names.insert(name.clone(), seg.span);
        }
        ParsedPattern::Capture { .. } => {
            reject_duplicate(&mut state.capture_span, seg.span, "captures")?;
        }
        ParsedPattern::RestCapture { .. } => {
            reject_duplicate(&mut state.rest_span, seg.span, "rest captures")?;
        }
        ParsedPattern::Glob => {
            reject_duplicate(&mut state.glob_span, seg.span, "globs")?;
        }
    }

    // Lookup patterns are validated during parsing (must be Capture with prefix/suffix).

    // Recursively validate children with fresh accumulators.
    validate_segments(&seg.sub_routes.iter().collect::<Vec<_>>())?;

    Ok(())
}

/// Reject a second occurrence of a pattern kind at the same nesting level.
///
/// If `first` is already `Some`, produces a compile error pointing at both the
/// original and duplicate spans — giving the user two locations to compare. Otherwise,
/// records the current span as the first occurrence for future checks.
fn reject_duplicate(first: &mut Option<Span>, current: Span, kind: &str) -> Result<()> {
    if let Some(prev) = *first {
        let mut err = syn::Error::new(current, format!("multiple {kind} at the same level are ambiguous"));
        err.combine(syn::Error::new(prev, "first defined here"));
        return Err(err);
    }
    *first = Some(current);
    Ok(())
}
