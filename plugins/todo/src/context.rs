//! Extension trait for accessing todo plugin services from [`ActivationContext`].

use std::sync::Arc;

use crate::provider::TodoState;

nyne::activation_context_ext! {
    /// Typed accessors for todo plugin services in [`ActivationContext`].
    pub trait TodoContextExt {
        /// Shared todo scanning state.
        todo_state -> Arc<TodoState>,
    }
}
