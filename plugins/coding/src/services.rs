use std::sync::Arc;

use nyne::dispatch::activation::ActivationContext;

use crate::config::CodingConfig;
use crate::lsp::manager::LspManager;
use crate::syntax::SyntaxRegistry;
use crate::syntax::analysis::AnalysisEngine;
use crate::syntax::decomposed::DecompositionCache;

/// Bundle of services registered by the coding plugin during activation.
///
/// Populated in [`Plugin::activate`] and inserted into the
/// [`ActivationContext`] `TypeMap` as a single entry. All provider code
/// retrieves services through [`Self::get`] instead of performing
/// individual type-erased lookups with per-site `expect` calls.
pub struct CodingServices {
    pub(crate) syntax: Arc<SyntaxRegistry>,
    pub(crate) lsp: Arc<LspManager>,
    pub(crate) decomposition: DecompositionCache,
    pub(crate) analysis: Arc<AnalysisEngine>,
    pub(crate) config: CodingConfig,
}

impl CodingServices {
    /// Retrieve the coding services from the activation context.
    ///
    /// # Panics
    ///
    /// Panics if the coding plugin has not been activated — a programming
    /// error in the plugin lifecycle.
    #[expect(clippy::expect_used, reason = "coding plugin activation is a lifecycle invariant")]
    pub(crate) fn get(ctx: &ActivationContext) -> &Self {
        ctx.get::<Self>()
            .expect("CodingServices missing — coding plugin was not activated")
    }
}
