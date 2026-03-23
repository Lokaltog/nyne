//! Convenience re-exports for plugin authors.

pub use std::sync::Arc;

pub use color_eyre::eyre::Result;

pub use crate::dispatch::activation::ActivationContext;
pub use crate::dispatch::context::RequestContext;
pub use crate::dispatch::invalidation::{EventSink, InvalidationEvent};
pub use crate::node::builtins::StaticContent;
pub use crate::node::{CachePolicy, VirtualNode};
pub use crate::plugin::{PLUGINS, Plugin};
pub use crate::provider::{Node, Nodes, Provider, ProviderId};
pub use crate::providers::companion_dir;
pub use crate::templates::{TemplateContent, TemplateView};
pub use crate::types::vfs_path::VfsPath;
