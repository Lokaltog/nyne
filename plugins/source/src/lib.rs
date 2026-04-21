//! nyne-source — syntax decomposition and code analysis for nyne.
//!
//! This crate provides the "source" plugin: tree-sitter parsing,
//! edit planning/splicing, and all providers that depend on
//! source-code understanding.

/// Batch edit staging and diff-based code actions.
pub(crate) mod edit;

/// FUSE providers that expose decomposed source code.
pub(crate) mod provider;

/// Tree-sitter parsing, symbol decomposition, and source analysis.
pub(crate) mod syntax;

pub(crate) mod context;
pub(crate) mod extensions;
pub(crate) mod paths;

// Public re-exports for downstream plugin crates.
pub use context::SourceContextExt;
pub use extensions::SourceExtensions;
pub use paths::SourcePaths;
pub use syntax::SyntaxRegistry;
pub use syntax::parser::TsNode;
pub use syntax::spec::Decomposer;
pub use util::{dominant_ext, parse_tag_suffix};

/// Shared resolver for lazily accessing decomposed source files.
pub(crate) mod fragment_resolver;

// Additional re-exports for downstream plugin crates.
pub use fragment_resolver::FragmentResolver;
pub use provider::syntax::SyntaxProvider;
pub use syntax::decomposed::{DecomposedSource, DecompositionCache, ResolvedFragment};
pub use syntax::fragment::Fragment;
pub use syntax::fs_mapping::split_disambiguator;
pub use syntax::view::{SYMBOL_TABLE_PARTIAL_KEY, SYMBOL_TABLE_PARTIAL_SRC, fragment_list};
pub use syntax::{find_fragment, find_fragment_at_line};

/// Provider utilities.
pub(crate) mod util;

/// Plugin registration and lifecycle implementation.
mod plugin;

/// Shared test utilities and stub contexts.
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
