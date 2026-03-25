use proc_macro2::TokenStream;
use quote::quote;
use syn::Result;

use super::parse::{FileEntry, FileModifier, LookupEntry, ParsedPattern, RouteEntry, RoutesInput, SegmentRoute};

/// Generate the complete `RouteTree` token stream from parsed and validated route input.
pub fn generate(input: &RoutesInput) -> Result<TokenStream> {
    let ty = &input.provider_ty;
    let route_calls = generate_entries(&input.entries, ty)?;

    Ok(quote! {
        {
            use ::nyne::dispatch::routing::builder::{RouteTreeBuilder, RouteNodeBuilder};
            use ::nyne::dispatch::routing::ctx::RouteCtx;
            use ::nyne::dispatch::routing::params::RouteParams;
            use ::nyne::node::VirtualNode;
            use ::nyne::provider::{Node, Nodes};

            RouteTreeBuilder::new()
                #(#route_calls)*
                .build()
        }
    })
}

/// Generate builder method calls for a list of route entries.
fn generate_entries(entries: &[RouteEntry], ty: &syn::Type) -> Result<Vec<TokenStream>> {
    let mut calls = Vec::new();
    for entry in entries {
        let tokens = match entry {
            RouteEntry::Segment(seg) => generate_segment(seg, ty)?,
            RouteEntry::Children(handler) => quote! {
                .children(|p: &#ty, ctx: &RouteCtx<'_>| p.#handler(ctx))
            },
            RouteEntry::Lookup(lookup) => generate_root_lookup(lookup, ty),
            RouteEntry::File(file) => generate_file(file),
        };
        calls.push(tokens);
    }
    Ok(calls)
}

/// Generate a `.route(...)` builder call for a single segment, including its children, lookups, and files.
fn generate_segment(seg: &SegmentRoute, ty: &syn::Type) -> Result<TokenStream> {
    let builder_ctor = pattern_to_builder(&seg.parsed_pattern);

    let children_call = seg.children_handler.as_ref().map(|handler| {
        quote! { .children(|p: &#ty, ctx: &RouteCtx<'_>| ::nyne::dispatch::routing::tree::IntoNodes::into_nodes(p.#handler(ctx))) }
    });

    let lookup_call = generate_lookup_closure(&seg.lookups, ty)?;

    let file_calls: Vec<TokenStream> = seg.files.iter().map(generate_file).collect();

    let sub_route_calls: Vec<TokenStream> = seg
        .sub_routes
        .iter()
        .filter_map(|entry| {
            if let RouteEntry::Segment(sub) = entry {
                Some(generate_segment(sub, ty))
            } else {
                None
            }
        })
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

/// Generate the `.lookup(...)` closure for a segment's lookup entries, combining pattern arms and an optional catch-all.
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
            LookupEntry::Pattern { parsed, handler, .. } => {
                pattern_arms.push(generate_lookup_pattern_arm(parsed, handler));
            }
        }
    }

    // Simple catch-all only
    if pattern_arms.is_empty()
        && let Some(handler) = catch_all
    {
        return Ok(Some(
            quote! { .lookup(|p: &#ty, ctx: &RouteCtx<'_>, name: &str| ::nyne::dispatch::routing::tree::IntoNode::into_node(p.#handler(ctx, name))) },
        ));
    }

    let fallback = if let Some(handler) = catch_all {
        quote! { ::nyne::dispatch::routing::tree::IntoNode::into_node(p.#handler(ctx, name)) }
    } else {
        quote! { Ok(None) }
    };

    Ok(Some(quote! {
        .lookup(|p: &#ty, ctx: &RouteCtx<'_>, name: &str| {
            #(#pattern_arms)*
            #fallback
        })
    }))
}

/// Generate an `if let` arm that strips a prefix/suffix from the lookup name and dispatches to the handler.
///
/// The pattern is pre-validated during parsing to be a `Capture` with at least a prefix or suffix.
fn generate_lookup_pattern_arm(pattern: &ParsedPattern, handler: &syn::Ident) -> TokenStream {
    let ParsedPattern::Capture { name, prefix, suffix } = pattern else {
        unreachable!("lookup patterns are validated as Capture during parsing")
    };

    // Build the match expression: strip prefix then suffix (or just one).
    let strip_expr = match (prefix, suffix) {
        (Some(pfx), Some(sfx)) => quote! {
            name.strip_prefix(#pfx).and_then(|s| s.strip_suffix(#sfx))
        },
        (Some(pfx), None) => quote! { name.strip_prefix(#pfx) },
        (None, Some(sfx)) => quote! { name.strip_suffix(#sfx) },
        (None, None) => unreachable!("lookup patterns are validated to have prefix or suffix during parsing"),
    };

    quote! {
        if let Some(__captured) = #strip_expr {
            if !__captured.is_empty() {
                let mut __params = ctx.route_params().clone();
                __params.insert_single(#name, __captured.to_owned());
                let __rctx = RouteCtx::new(ctx.request(), __params);
                return ::nyne::dispatch::routing::tree::IntoNode::into_node(p.#handler(&__rctx));
            }
        }
    }
}

/// Convert `Option<String>` to `Some("...")` / `None` token stream.
fn option_to_tokens(opt: Option<&String>) -> TokenStream {
    if let Some(s) = opt {
        quote! { Some(#s) }
    } else {
        quote! { None }
    }
}

/// Generate a root-level `.lookup(...)` call on `RouteTreeBuilder`.
fn generate_root_lookup(lookup: &LookupEntry, ty: &syn::Type) -> TokenStream {
    match lookup {
        LookupEntry::CatchAll { handler } => quote! {
            .lookup(|p: &#ty, ctx: &RouteCtx<'_>, name: &str| ::nyne::dispatch::routing::tree::IntoNode::into_node(p.#handler(ctx, name)))
        },
        LookupEntry::Pattern { parsed, handler, .. } => {
            let arm = generate_lookup_pattern_arm(parsed, handler);
            quote! {
                .lookup(|p: &#ty, ctx: &RouteCtx<'_>, name: &str| {
                    #arm
                    Ok(None)
                })
            }
        }
    }
}

/// Generate a `.file(...)` builder call for a static file entry with its modifiers.
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
