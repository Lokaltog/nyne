//! Resolver trait and implementation for Router with recursion depth guard.

//! Resolver trait for virtual path-to-node resolution with recursion guard.
//!
//! The [`Resolver`] trait is the controlled escape hatch that lets nodes and
//! providers access the router's resolution and lookup capabilities. Without it,
//! a node's `read()` handler would have no way to discover sibling or child
//! nodes (e.g., an `OVERVIEW.md` that lists all symbols in a directory).
//!
//! Because resolution can trigger further resolution (a node's content may
//! depend on resolving another directory), a thread-local depth counter
//! prevents infinite recursion. The limit ([`MAX_RESOLVER_DEPTH`]) is generous
//! enough for legitimate chains (e.g., companion -> symbols -> nested impl)
//! but catches bugs quickly.
//!
//! This is an **interface module** — the trait may be imported from any tier.

use std::cell::Cell;
use std::sync::Arc;

use color_eyre::eyre::{Result, bail};

use super::cache::CachedNodeKind;
use super::router::Router;
use crate::node::VirtualNode;
use crate::types::ProcessVisibility;
use crate::types::vfs_path::VfsPath;

/// Resolves virtual paths to nodes.
///
/// Gives nodes access to the router's resolution and lookup capabilities.
/// This is the controlled escape hatch for compound operations that span
/// multiple nodes or providers. Implemented by the Router.
pub trait Resolver: Send + Sync {
    /// Resolve all visible nodes at a directory path.
    /// Goes through the same cache + provider resolution as external access.
    fn resolve(&self, path: &VfsPath) -> Result<Vec<Arc<VirtualNode>>>;

    /// Look up a single node by full path.
    /// Tries L1 cache first, then provider `lookup()` as fallback.
    fn lookup(&self, path: &VfsPath) -> Result<Option<Arc<VirtualNode>>>;
}

/// Maximum resolver recursion depth.
///
/// Prevents infinite loops when a node's `read()` handler calls
/// `resolver.lookup()` which triggers `resolve()` on another directory
/// whose node calls `resolver.lookup()` again, etc.
const MAX_RESOLVER_DEPTH: u8 = 8;

thread_local! {
    static RESOLVER_DEPTH: Cell<u8> = const { Cell::new(0) };
}

/// RAII guard that increments the resolver recursion depth on creation
/// and decrements it on drop.
struct ResolverDepthGuard;

/// RAII depth guard construction.
impl ResolverDepthGuard {
    /// Increment the thread-local depth counter, failing if the limit is exceeded.
    fn enter() -> Result<Self> {
        RESOLVER_DEPTH.with(|d| {
            let next = d.get().checked_add(1).filter(|&n| n <= MAX_RESOLVER_DEPTH);
            let Some(next) = next else {
                bail!("resolver recursion depth exceeded (max {MAX_RESOLVER_DEPTH})");
            };
            d.set(next);
            Ok(Self)
        })
    }
}

/// Decrements the thread-local depth counter on drop.
impl Drop for ResolverDepthGuard {
    /// Decrement the resolver recursion depth.
    fn drop(&mut self) { RESOLVER_DEPTH.with(|d| d.set(d.get() - 1)); }
}

/// [`Resolver`] implementation for [`Router`] with recursion depth guarding.
impl Resolver for Router {
    /// Resolve all visible virtual nodes in a directory.
    fn resolve(&self, path: &VfsPath) -> Result<Vec<Arc<VirtualNode>>> {
        let _guard = ResolverDepthGuard::enter()?;
        let ctx = self.make_request_context(path);
        self.ensure_resolved(&ctx)?;

        Ok(self
            .cache
            .get(path)
            .map(|handle| {
                let dir = handle.read();
                dir.readdir_entries(ProcessVisibility::Default)
                    .filter_map(|(_, cn)| match &cn.kind {
                        CachedNodeKind::Virtual { node, .. } => Some(Arc::clone(node)),
                        CachedNodeKind::Real { .. } => None,
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Look up a single virtual node by its full path.
    fn lookup(&self, path: &VfsPath) -> Result<Option<Arc<VirtualNode>>> {
        let _guard = ResolverDepthGuard::enter()?;
        let Some(parent) = path.parent() else {
            return Ok(None);
        };
        let Some(name) = path.name() else {
            return Ok(None);
        };
        let ctx = self.make_request_context(&parent);
        self.lookup_name(name, &ctx)?;

        // Return Arc clone from cache.
        Ok(self.cache.get(&parent).and_then(|handle| {
            let dir = handle.read();
            dir.get(name).and_then(|cn| match &cn.kind {
                CachedNodeKind::Virtual { node, .. } => Some(Arc::clone(node)),
                CachedNodeKind::Real { .. } => None,
            })
        }))
    }
}
