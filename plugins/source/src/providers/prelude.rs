//! Common imports for provider implementations in nyne-coding.
//!
//! Re-exports the core dispatch, node, and provider types that virtually every
//! provider module needs, so individual files can `use super::prelude::*` instead
//! of repeating the same import block.

pub use std::sync::Arc;

pub use nyne::dispatch::activation::ActivationContext;
pub use nyne::dispatch::context::RequestContext;
pub use nyne::node::{CachePolicy, VirtualNode};
pub use nyne::provider::{Node, Nodes, Provider, ProviderId};
pub use nyne::types::vfs_path::VfsPath;
