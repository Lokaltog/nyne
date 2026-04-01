//! Extension trait for accessing companion plugin services from [`ActivationContext`].

use crate::extensions::CompanionExtensions;

nyne::activation_context_ext! {
    /// Typed accessors for companion plugin services in [`ActivationContext`].
    pub trait CompanionContextExt {
        /// Companion extension routes contributed by downstream plugins.
        companion_extensions -> CompanionExtensions,
        /// Mutable access to companion extensions (initializes if absent).
        mut companion_extensions_mut -> CompanionExtensions,
    }
}
