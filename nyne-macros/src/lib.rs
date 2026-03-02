mod routes;

use proc_macro::TokenStream;

/// Declarative route tree for nyne providers.
///
/// Parses a tree DSL and generates `RouteTreeBuilder` calls. See
/// `dispatch/routing/` for the `RouteTree` type this macro produces.
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
#[proc_macro]
pub fn routes(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as routes::parse::RoutesInput);
    match routes::expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
