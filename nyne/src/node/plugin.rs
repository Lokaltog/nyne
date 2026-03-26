//! Node plugin trait for extending node construction with parametric derivation.
//!
//! A [`NodePlugin`] lets a single base node spawn an open-ended family of
//! derived nodes without pre-registering them. The dispatch layer tries
//! plugins on lookup misses, so derived nodes are created lazily and
//! transparently — callers see them as regular virtual nodes.
//!
//! This is distinct from the top-level [`Plugin`](crate::plugin::Plugin)
//! trait which manages provider lifecycle. `NodePlugin` operates at the
//! individual node level within a provider's namespace.

use std::sync::Arc;

use color_eyre::eyre::Result;

use super::VirtualNode;
use crate::dispatch::context::RequestContext;

/// A composable behavior that derives parametric virtual nodes from a base node.
///
/// Plugins enable patterns like `BLAME.md:5-20` (line slicing) where a
/// "variant" node is derived from a "base" node. The dispatch layer calls
/// plugins generically — no per-plugin logic in dispatch.
///
/// # Implementing a plugin
///
/// ```rust,ignore
/// struct LineSlice;
///
/// impl NodePlugin for LineSlice {
///     fn derive(&self, base: &Arc<VirtualNode>, name: &str, ctx: &RequestContext<'_>)
///         -> Result<Option<VirtualNode>>
///     {
///         // Try to parse "BLAME.md:5-20" from name
///         // If match: return Ok(Some(derived_node))
///         // If no match: return Ok(None)
///     }
/// }
/// ```
pub trait NodePlugin: Send + Sync {
    /// Try to derive a node for `name` from the base node.
    ///
    /// Called by the dispatch layer when a lookup misses L1 and before
    /// falling back to `Provider::lookup()`. Return `Ok(None)` to decline.
    fn derive(&self, base: &Arc<VirtualNode>, name: &str, ctx: &RequestContext<'_>) -> Result<Option<VirtualNode>>;
}
