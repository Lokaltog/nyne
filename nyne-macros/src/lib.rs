//! Procedural macros for the nyne virtual filesystem.
//!
//! This crate provides the [`routes!`] macro, which lets provider authors declare
//! their virtual directory hierarchy as a compile-time DSL rather than hand-building
//! [`RouteTreeBuilder`] chains. The macro is the sole public export — all supporting
//! code (parsing, validation, code generation) lives in the private [`routes`] module.

/// Route tree macro internals: parsing, validation, and code generation.
///
/// This module is split into three phases that mirror a traditional compiler pipeline:
///
/// 1. **Parsing** (`parse`) — converts the `routes!` token stream into a typed AST
///    (`RoutesInput`, `RouteEntry`, `SegmentRoute`, etc.).
/// 2. **Validation** (`mod.rs`) — enforces structural constraints like no duplicate exact
///    segments or multiple captures at the same nesting level.
/// 3. **Code generation** (`codegen`) — lowers the validated AST into `RouteTreeBuilder`
///    method calls that construct the runtime route tree.
mod routes;

use proc_macro::TokenStream;

/// Declarative route tree for nyne providers.
///
/// This is the primary macro for defining a provider's virtual filesystem layout.
/// It parses a tree DSL and generates [`RouteTreeBuilder`] calls that construct the
/// runtime route tree used by the dispatch layer. See `dispatch/routing/` for the
/// [`RouteTree`] type this macro produces.
///
/// The first argument is the provider type (typically `Self`), which determines the
/// receiver for all handler method calls. The second argument is a braced block of
/// route entries that define the virtual directory hierarchy.
///
/// # Syntax
///
/// ```rust,ignore
/// routes!(Self, {
///     "segment" => children_handler {
///         "{capture}@" => nested_handler,
///         lookup "{ref}.ext" => lookup_by_pattern,
///         lookup(catch_all_lookup),
///         file("name.md", content_expr),
///     }
///     "**" => lookup(glob_handler),
/// })
/// ```
///
/// # Entry types
///
/// - **Segment** (`"pattern"`) — a directory node. Patterns can be exact literals,
///   `{capture}` with optional prefix/suffix, `{..rest}` for rest captures, or `"**"` for globs.
///   Use `=> handler` to attach a children handler, and `{ ... }` to nest sub-routes.
/// - **Lookup** — dynamic name resolution. `lookup(handler)` is a catch-all;
///   `lookup "pre{name}suf" => handler` matches names with the given prefix/suffix.
/// - **File** — a static virtual file. `file("name", expr)` with optional `.no_cache()`,
///   `.hidden()`, or `.sliceable()` modifiers.
/// - **Children** — `children(handler)` at root level delegates child listing.
///
/// # Errors
///
/// Produces compile-time errors for structural violations such as duplicate exact
/// segments at the same level, multiple captures or globs at the same level, or
/// malformed capture patterns.
#[proc_macro]
pub fn routes(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as routes::parse::RoutesInput);
    match routes::expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
