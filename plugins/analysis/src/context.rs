//! Extension trait for accessing analysis plugin services from [`ActivationContext`].

use std::sync::Arc;

use crate::engine::Engine;

nyne::activation_context_ext! {
    /// Typed accessors for analysis plugin services in [`ActivationContext`].
    pub trait AnalysisContextExt {
        /// The static analysis engine (code smell detection).
        analysis_engine -> Arc<Engine>,
    }
}
