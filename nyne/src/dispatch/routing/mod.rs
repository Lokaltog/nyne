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
