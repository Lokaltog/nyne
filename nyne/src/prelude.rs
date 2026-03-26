//! Convenience re-exports for plugin authors.
//!
//! Import `use nyne::prelude::*` in provider implementations to get the most
//! commonly needed types without listing each one individually. This includes
//! the [`Provider`] trait, virtual node types, dispatch context types, error
//! handling utilities, and the plugin registration machinery.
//!
//! This prelude is intentionally narrow — it covers the "happy path" for writing
//! a typical provider. Types needed only for advanced use cases (middleware,
//! capabilities, conflict resolution) should be imported explicitly from their
//! respective modules.

pub use std::sync::Arc;

pub use color_eyre::eyre::Result;

pub use crate::dispatch::activation::ActivationContext;
pub use crate::dispatch::context::RequestContext;
pub use crate::dispatch::invalidation::{EventSink, InvalidationEvent};
pub use crate::err::io_err;
pub use crate::node::builtins::StaticContent;
pub use crate::node::{CachePolicy, VirtualNode};
pub use crate::plugin::{PLUGINS, Plugin};
pub use crate::provider::{Node, Nodes, Provider, ProviderId};
pub use crate::providers::companion_dir;
pub use crate::templates::{TemplateContent, TemplateView};
pub use crate::types::vfs_path::VfsPath;
