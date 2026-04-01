mod cached;

use std::path::PathBuf;
use std::sync::Arc;

use cached::{CachedNode, CachedResult, source_from_request, wrap_readable};
use color_eyre::eyre::Result;
use nyne::router::{GenCache, GenerationMap, InvalidationEvent, Next, NodeAccumulator, Op, Provider, Request};
use tracing::trace;

/// Resolution cache middleware.
///
/// Sits early in the chain (priority -50) to short-circuit readdir and lookup
/// on cache hits. Mutation ops pass through and invalidate affected paths.
/// Uses generation-based staleness detection via [`GenCache`].
pub struct CacheProvider {
    readdir_cache: GenCache<PathBuf, CachedResult<Vec<CachedNode>>>,
    lookup_cache: GenCache<(PathBuf, String), CachedResult<Option<CachedNode>>>,
    generations: Arc<GenerationMap>,
}

nyne::define_provider!(CacheProvider, "cache", priority: -50);

impl CacheProvider {
    pub fn new(generations: Arc<GenerationMap>) -> Self {
        Self {
            readdir_cache: GenCache::new(Arc::clone(&generations)),
            lookup_cache: GenCache::new(Arc::clone(&generations)),
            generations,
        }
    }

    /// Invalidate all cached entries for a path (both readdir and lookup).
    fn invalidate(&self, path: &PathBuf) {
        self.readdir_cache.invalidate(path);
        // Lookup entries are keyed by (dir, name) -- we can't enumerate all
        // names for a dir. Rely on generation-based staleness for lookup
        // entries; explicit invalidation only for readdir.
        trace!(path = %path.display(), "cache invalidated");
    }

    /// Wrap a named node's readable in-place and clone for cache storage.
    fn wrap_and_lookup(nodes: &mut NodeAccumulator, name: &str) -> Option<CachedNode> {
        if let Some(node) = nodes.find_mut(name) {
            wrap_readable(node);
        }
        nodes.find(name).map(|n| CachedNode(n.clone()))
    }

    /// Wrap all readables in the accumulator in-place and snapshot for cache storage.
    fn wrap_and_snapshot(nodes: &mut NodeAccumulator) -> Vec<CachedNode> {
        for node in nodes.iter_mut() {
            wrap_readable(node);
        }
        nodes.iter().map(|n| CachedNode(n.clone())).collect()
    }
}

impl Provider for CacheProvider {
    fn accept(&self, req: &mut Request, next: &Next) -> Result<()> {
        match req.op().clone() {
            Op::Readdir => {
                let path = req.path().to_owned();

                let cached = self.readdir_cache.get_or_compute(path.clone(), || {
                    next.run(req).ok();
                    // Derive source AFTER next.run — companion state is now set.
                    (
                        CachedResult {
                            value: Self::wrap_and_snapshot(&mut req.nodes),
                            state: req.clone_state(),
                        },
                        source_from_request(req),
                    )
                });

                // Only populate from cache if we didn't just run next
                // (get_or_compute runs compute in-place on miss, which
                // already populated req.nodes via next.run).
                if !req.nodes.is_empty() {
                    return Ok(());
                }

                trace!(path = %path.display(), entries = cached.value.len(), "cache hit");
                req.restore_state(cached.state);
                for node in cached.value {
                    req.nodes.add(node.0);
                }
                Ok(())
            }
            Op::Lookup { ref name } => {
                let name = name.clone();
                let cached = self
                    .lookup_cache
                    .get_or_compute((req.path().to_owned(), name.clone()), || {
                        next.run(req).ok();
                        (
                            CachedResult {
                                value: Self::wrap_and_lookup(&mut req.nodes, &name),
                                state: req.clone_state(),
                            },
                            source_from_request(req),
                        )
                    });

                if !req.nodes.is_empty() {
                    return Ok(());
                }

                if let Some(node) = cached.value {
                    req.restore_state(cached.state);
                    trace!(path = %req.path().display(), name = %node.0.name(), "lookup cache hit");
                    req.nodes.add(node.0);
                }
                Ok(())
            }
            // Mutations: never cached -- always dispatch, then invalidate.
            Op::Create { .. } | Op::Mkdir { .. } | Op::Remove { .. } => {
                next.run(req)?;
                let path = req.path().to_owned();
                self.invalidate(&path);
                self.generations.bump(&source_from_request(req));
                Ok(())
            }
            Op::Rename { ref target_dir, .. } => {
                let target = target_dir.clone();
                next.run(req)?;
                let path = req.path().to_owned();
                self.invalidate(&path);
                self.invalidate(&target);
                self.generations.bump(&source_from_request(req));
                Ok(())
            }
        }
    }

    fn on_change(&self, changed: &[PathBuf]) -> Vec<InvalidationEvent> {
        for path in changed {
            self.generations.bump(path);
        }
        Vec::new()
    }
}
#[cfg(test)]
mod tests;
