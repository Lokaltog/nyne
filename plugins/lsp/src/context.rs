//! Extension trait for accessing LSP plugin services from [`ActivationContext`].

use std::sync::Arc;

use crate::provider::LspState;
use crate::session::manager::Manager;

nyne::activation_context_ext! {
    /// Typed accessors for LSP plugin services in [`ActivationContext`].
    pub trait LspContextExt {
        /// The LSP session manager (server lifecycle and query dispatch).
        lsp_manager -> Arc<Manager>,
        /// Shared LSP state (configuration, path resolver, handles).
        lsp_state -> Arc<LspState>,
    }
}
