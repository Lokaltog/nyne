//! Common imports for provider implementations.
//!
//! Providers start with `use super::prelude::*;` instead of manually
//! importing the 6+ types every provider needs.

pub use std::sync::Arc;

pub use crate::dispatch::activation::ActivationContext;
pub use crate::dispatch::context::RequestContext;
pub use crate::node::{CachePolicy, VirtualNode};
pub use crate::provider::{Node, Nodes, Provider, ProviderId};
pub use crate::types::vfs_path::VfsPath;
