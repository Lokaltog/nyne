//! Extension trait for accessing source plugin services from [`ActivationContext`].

use std::sync::Arc;

use crate::edit::staging::EditStaging;
use crate::extensions::SourceExtensions;
use crate::paths::SourcePaths;
use crate::syntax::SyntaxRegistry;
use crate::syntax::decomposed::DecompositionCache;

nyne::activation_context_ext! {
    /// Typed accessors for source plugin services in [`ActivationContext`].
    ///
    /// Implemented on `ActivationContext` so downstream plugins can replace
    /// opaque `ctx.get::<T>()` calls with named methods.
    pub trait SourceContextExt {
        /// The shared syntax registry (tree-sitter grammars and language config).
        syntax_registry -> Arc<SyntaxRegistry>,
        /// Source file path configuration.
        source_paths -> Arc<SourcePaths>,
        /// The decomposition cache for parsed file contents.
        decomposition_cache -> DecompositionCache,
        /// The batch edit staging area.
        edit_staging -> EditStaging,
        /// Source extension routes contributed by downstream plugins.
        source_extensions -> SourceExtensions,
        /// Mutable access to source extensions (initializes if absent).
        mut source_extensions_mut -> SourceExtensions,
    }
}
