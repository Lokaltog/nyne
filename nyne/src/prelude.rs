//! Convenience re-exports for plugin authors.
//!
//! Import `use nyne::prelude::*` in provider implementations to get the most
//! commonly needed types without listing each one individually.
//!
//! This prelude is intentionally narrow — it covers the "happy path" for writing
//! a typical provider. Types needed only for advanced use cases should be
//! imported explicitly from their respective modules.

pub use std::sync::Arc;

pub use color_eyre::eyre::Result;

pub use crate::config::PluginConfig;
pub use crate::dispatch::activation::ActivationContext;
pub use crate::err::io_err;
pub use crate::plugin::{PLUGINS, Plugin};
pub use crate::router::{
    AffectedFiles, CachePolicy, LazyReadable, NamedNode, Next, Node, NodeKind, Op, Provider, ProviderId, ProviderMeta,
    ReadContext, Readable, Request, RouteCtx, RouteTree, Writable,
};
pub use crate::templates::{TemplateContent, TemplateView};
pub use crate::types::Timestamps;
