//! Code generation phase of the `routes!` macro.
//!
//! Transforms the validated [`parse`](super::parse) AST into a token stream of
//! [`RouteTreeBuilder`] method calls. Each AST node type has a dedicated generator
//! function that emits the corresponding builder method — the structure of this module
//! directly mirrors the structure of the AST types.

use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::Result;

use super::parse::{FileEntry, FileModifier, LookupEntry, ParsedPattern, RouteEntry, RoutesInput, SegmentRoute};

/// Generate the complete `RouteTree` token stream from parsed and validated route input.
///
/// This is the top-level entry point for code generation. It wraps all generated route
/// calls in the necessary `use` imports (builder types, context types, node traits) and
/// a `RouteTreeBuilder::new() ... .build()` expression. The resulting token stream is a
/// complete expression that evaluates to a `RouteTree`.
///
/// The `provider_ty` from the input is threaded through to every handler closure so that
/// method calls resolve against the correct provider type.
pub fn generate(input: &RoutesInput) -> Result<TokenStream> {
    let ty = &input.provider_ty;
    let route_calls = generate_entries(&input.entries, ty)?;

    Ok(quote! {
        {
            use ::nyne::dispatch::routing::builder::{RouteTreeBuilder, RouteNodeBuilder};
            use ::nyne::dispatch::routing::ctx::RouteCtx;
            use ::nyne::dispatch::routing::params::RouteParams;
            use ::nyne::dispatch::routing::tree::{IntoNode, IntoNodes};
            use ::nyne::node::VirtualNode;
            use ::nyne::provider::{Node, Nodes};

            RouteTreeBuilder::new()
                #(#route_calls)*
                .build()
        }
    })
}

/// Generate builder method calls for a list of route entries.
///
/// Dispatches each entry to its specialized generator based on variant:
/// segments become `.route(...)` calls, children become `.children(...)` closures,
/// lookups become `.lookup(...)` closures, and files become `.file(...)` calls.
/// The ordering of entries is preserved in the generated output.
fn generate_entries(entries: &[RouteEntry], ty: &syn::Type) -> Result<Vec<TokenStream>> {
    let mut calls = Vec::new();
    for entry in entries {
        let tokens = match entry {
            RouteEntry::Segment(seg) => generate_segment(seg, ty)?,
            RouteEntry::Children(handler) => {
                let head = children_closure_head(ty);
                quote! { .children(#head p.#handler(ctx)) }
            }
            RouteEntry::Lookup(lookup) => generate_root_lookup(lookup, ty),
            RouteEntry::File(file) => generate_file(file),
        };
        calls.push(tokens);
    }
    Ok(calls)
}
/// Emit the parameter list for a `.children(...)` closure: `|p: &Ty, ctx: &RouteCtx<'_>|`.
fn children_closure_head(ty: &syn::Type) -> TokenStream {
    quote! { |p: &#ty, ctx: &RouteCtx<'_>| }
}

/// Emit the parameter list for a `.lookup(...)` closure: `|p: &Ty, ctx: &RouteCtx<'_>, name: &str|`.
fn lookup_closure_head(ty: &syn::Type) -> TokenStream {
    quote! { |p: &#ty, ctx: &RouteCtx<'_>, name: &str| }
}

/// Generate a `.route(...)` builder call for a single segment.
///
/// Assembles all sub-parts of a segment into a single chained builder expression:
/// the pattern constructor, optional `no_emit` flag, optional children handler,
/// lookup closure, static files, and recursively generated sub-routes. This mirrors
/// the `RouteNodeBuilder` fluent API where each part is an optional chained method.
fn generate_segment(seg: &SegmentRoute, ty: &syn::Type) -> Result<TokenStream> {
    let builder_ctor = pattern_to_builder(&seg.parsed_pattern);

    let children_call = seg.children_handler.as_ref().map(|handler| {
        let head = children_closure_head(ty);
        quote! { .children(#head IntoNodes::into_nodes(p.#handler(ctx))) }
    });

    let lookup_call = generate_lookup_closure(&seg.lookups, ty)?;

    let file_calls: Vec<TokenStream> = seg.files.iter().map(generate_file).collect();

    let sub_route_calls: Vec<TokenStream> = seg
        .sub_routes
        .iter()
        .map(|sub| generate_segment(sub, ty))
        .collect::<Result<_>>()?;

    let no_emit_call = seg.no_emit.then(|| quote! { .no_emit() });

    Ok(quote! {
        .route(
            #builder_ctor
            #no_emit_call
            #children_call
            #lookup_call
            #(#file_calls)*
            #(#sub_route_calls)*
        )
    })
}

/// Convert a parsed segment pattern into its `RouteNodeBuilder` constructor call.
///
/// Each [`ParsedPattern`] variant maps to a specific builder constructor:
/// - `Exact` -> `RouteNodeBuilder::exact("literal")`
/// - `Glob` -> `RouteNodeBuilder::glob()`
/// - `Capture` -> `RouteNodeBuilder::capture(name, prefix, suffix)`
/// - `RestCapture` -> `RouteNodeBuilder::rest_capture(name, suffix)`
///
/// Prefix and suffix are emitted as `Option<&str>` using [`option_to_tokens`].
fn pattern_to_builder(pattern: &ParsedPattern) -> TokenStream {
    match pattern {
        ParsedPattern::Exact(s) => quote! { RouteNodeBuilder::exact(#s) },
        ParsedPattern::Glob => quote! { RouteNodeBuilder::glob() },
        ParsedPattern::Capture { name, prefix, suffix } => {
            let prefix_expr = option_to_tokens(prefix.as_ref());
            let suffix_expr = option_to_tokens(suffix.as_ref());
            quote! { RouteNodeBuilder::capture(#name, #prefix_expr, #suffix_expr) }
        }
        ParsedPattern::RestCapture { name, suffix } => {
            let suffix_expr = option_to_tokens(suffix.as_ref());
            quote! { RouteNodeBuilder::rest_capture(#name, #suffix_expr) }
        }
    }
}

/// Generate the `.lookup(...)` closure for a segment's lookup entries.
///
/// Combines pattern-based lookup arms and an optional catch-all into a single closure.
/// The generated closure receives `(provider, ctx, name)` and tries each pattern arm
/// in order (using prefix/suffix stripping), falling back to the catch-all handler or
/// `Ok(None)` if no pattern matches.
///
/// When only a catch-all is present (no pattern arms), the closure is simplified to a
/// direct delegation without any conditional logic.
///
/// Returns `None` if the lookup list is empty (no `.lookup()` call needed).
fn generate_lookup_closure(lookups: &[LookupEntry], ty: &syn::Type) -> Result<Option<TokenStream>> {
    if lookups.is_empty() {
        return Ok(None);
    }

    let mut pattern_arms = Vec::new();
    let mut catch_all: Option<&syn::Ident> = None;

    for lookup in lookups {
        match lookup {
            LookupEntry::CatchAll { handler } => {
                if catch_all.is_some() {
                    return Err(syn::Error::new_spanned(handler, "duplicate catch-all lookup handler"));
                }
                catch_all = Some(handler);
            }
            LookupEntry::Pattern { pattern, handler, .. } => {
                pattern_arms.push(generate_lookup_pattern_arm(pattern, handler));
            }
        }
    }

    // Simple catch-all only
    if pattern_arms.is_empty()
        && let Some(handler) = catch_all
    {
        let head = lookup_closure_head(ty);
        return Ok(Some(
            quote! { .lookup(#head IntoNode::into_node(p.#handler(ctx, name))) },
        ));
    }

    let fallback = if let Some(handler) = catch_all {
        quote! { IntoNode::into_node(p.#handler(ctx, name)) }
    } else {
        quote! { Ok(None) }
    };

    let head = lookup_closure_head(ty);
    Ok(Some(quote! {
        .lookup(#head {
            #(#pattern_arms)*
            #fallback
        })
    }))
}

/// Generate an `if let` arm that strips a prefix/suffix from the lookup name and dispatches.
///
/// Produces an `if let Some(_nyne_captured) = name.strip_prefix(...).and_then(...)`
/// block that extracts the captured portion of a lookup name. When matched, it clones
/// the current route params, inserts the captured value under the pattern's capture
/// name, and calls the handler with the augmented context.
///
/// The guard `!_nyne_captured.is_empty()` prevents matching when the capture group
/// would be empty (e.g., a bare prefix with nothing after it), which would be ambiguous.
///
/// The pattern is pre-validated during parsing to be a `Capture` with at least a prefix
/// or suffix — the `Exact`/`Glob`/`RestCapture` cases are internal errors.
fn generate_lookup_pattern_arm(pattern: &ParsedPattern, handler: &syn::Ident) -> TokenStream {
    let ParsedPattern::Capture { name, prefix, suffix } = pattern else {
        unreachable!("lookup pattern must be a Capture variant (validated during parsing)");
    };

    // Build the match expression: strip prefix then suffix (or just one).
    let strip_expr = match (prefix, suffix) {
        (Some(pfx), Some(sfx)) => quote! {
            name.strip_prefix(#pfx).and_then(|s| s.strip_suffix(#sfx))
        },
        (Some(pfx), None) => quote! { name.strip_prefix(#pfx) },
        (None, Some(sfx)) => quote! { name.strip_suffix(#sfx) },
        (None, None) => {
            unreachable!("lookup capture pattern must have a prefix or suffix (validated during parsing)");
        }
    };

    let mixed = Span::mixed_site();
    let captured = Ident::new("_nyne_captured", mixed);
    let params = Ident::new("_nyne_params", mixed);
    let rctx = Ident::new("_nyne_rctx", mixed);

    quote! {
        if let Some(#captured) = #strip_expr {
            if !#captured.is_empty() {
                let mut #params = ctx.route_params().clone();
                #params.insert_single(#name, #captured.to_owned());
                let #rctx = RouteCtx::new(ctx.request(), #params);
                return IntoNode::into_node(p.#handler(&#rctx));
            }
        }
    }
}

/// Convert an `Option<String>` reference to its token stream representation.
///
/// Produces `Some("value")` or `None` as tokens, used to emit prefix/suffix
/// arguments in capture and rest-capture pattern constructors.
fn option_to_tokens(opt: Option<&String>) -> TokenStream {
    if let Some(s) = opt {
        quote! { Some(#s) }
    } else {
        quote! { None }
    }
}

/// Generate a root-level `.lookup(...)` call on `RouteTreeBuilder`.
///
/// Unlike segment-level lookups (which are combined into a single closure by
/// [`generate_lookup_closure`]), root lookups appear as standalone `.lookup()`
/// calls on the top-level builder. A catch-all directly delegates to the handler;
/// a pattern-based lookup wraps the pattern arm with an `Ok(None)` fallback.
fn generate_root_lookup(lookup: &LookupEntry, ty: &syn::Type) -> TokenStream {
    let head = lookup_closure_head(ty);
    match lookup {
        LookupEntry::CatchAll { handler } => quote! {
            .lookup(#head IntoNode::into_node(p.#handler(ctx, name)))
        },
        LookupEntry::Pattern { pattern, handler, .. } => {
            let arm = generate_lookup_pattern_arm(pattern, handler);
            quote! { .lookup(#head { #arm Ok(None) }) }
        }
    }
}

/// Generate a `.file(...)` builder call for a static file entry.
///
/// Produces a `.file(name, || VirtualNode::file(name, content))` call with any
/// chained modifiers (`.no_cache()`, `.hidden()`, `.sliceable()`) appended. The
/// content expression is captured in a `move` closure so it can reference provider
/// fields or other state at construction time.
fn generate_file(file: &FileEntry) -> TokenStream {
    let name = &file.name;
    let content = &file.content;
    let modifiers: Vec<TokenStream> = file
        .modifiers
        .iter()
        .map(|m| match m {
            FileModifier::NoCache => quote! { .no_cache() },
            FileModifier::Hidden => quote! { .hidden() },
            FileModifier::Sliceable => quote! { .sliceable() },
        })
        .collect();
    quote! {
        .file(#name, move || VirtualNode::file(#name, #content) #(#modifiers)*)
    }
}
