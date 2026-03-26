use std::sync::Arc;

use nyne::dispatch::activation::ActivationContext;

use crate::config::SourceConfig;
use crate::syntax::SyntaxRegistry;
use crate::syntax::decomposed::DecompositionCache;

/// Bundle of services registered by the source plugin during activation.
///
/// Populated in [`Plugin::activate`] and inserted into the
/// [`ActivationContext`] `TypeMap` as a single entry. All provider code
/// retrieves services through [`Self::get`] instead of performing
/// individual type-erased lookups with per-site `expect` calls.
///
/// Bundling avoids the fragility of N separate `TypeMap` insertions
/// (where forgetting one causes a runtime panic at an arbitrary call site)
/// and makes the plugin's service surface explicit in one place.
pub struct SourceServices {
    /// Global tree-sitter grammar registry shared across all decompositions.
    pub syntax: Arc<SyntaxRegistry>,
    /// Caches parsed decompositions keyed by file path and content hash.
    pub decomposition: DecompositionCache,
    /// Resolved plugin configuration.
    pub config: SourceConfig,
}

impl SourceServices {
    /// Retrieve the source services from the activation context.
    ///
    /// # Panics
    ///
    /// Panics if the source plugin has not been activated — a programming
    /// error in the plugin lifecycle.
    #[expect(clippy::expect_used, reason = "source plugin activation is a lifecycle invariant")]
    pub fn get(ctx: &ActivationContext) -> &Self {
        ctx.get::<Self>()
            .expect("SourceServices missing — source plugin was not activated")
    }

    /// Retrieve the source services from the activation context, if present.
    ///
    /// Returns `None` if the source plugin has not been activated.
    pub fn try_get(ctx: &ActivationContext) -> Option<&Self> { ctx.get::<Self>() }
}
