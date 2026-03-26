//! URL-style route tree for dispatching VFS path lookups to provider handlers.
//!
//! This module implements a domain-agnostic hierarchical router inspired by
//! web frameworks (axum, Hono). Providers declare route trees with segment
//! matchers (exact, capture, rest-capture, glob) and the router walks the
//! VFS path to find matching handlers for `children` (readdir) and `lookup`
//! (single-name) operations.
//!
//! The route tree is the primary abstraction that lets providers define their
//! virtual filesystem namespace declaratively via the `routes!` proc-macro
//! (or the [`builder`] API directly), rather than implementing raw
//! `children`/`lookup` methods with manual path parsing.

/// Fluent builder API for constructing route trees.
pub mod builder;
/// Route handler context with captured parameters.
pub mod ctx;
/// Accumulated route captures from segment matching.
pub mod params;
/// Segment pattern types and matching logic.
pub mod segment;
/// Route tree structure, dispatch, and matching algorithms.
pub mod tree;

/// Unit tests for route matching, dispatch, and tree construction.
#[cfg(test)]
mod tests;
