pub(crate) mod context;
pub(crate) mod extensions;
/// Companion middleware — strips the configurable companion suffix and sets
/// `Companion` state for downstream providers.
pub(crate) mod provider;

pub use context::CompanionContextExt;
pub use extensions::CompanionExtensions;
pub use provider::{Companion, CompanionProvider, CompanionRequest};

mod plugin;
