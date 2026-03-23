//! Resolution and name lookup operations.

use std::collections::HashSet;
use std::sync::Arc;

use color_eyre::eyre::Result;

use super::{ResolvedInode, Router};
use crate::dispatch::cache::{CachedNodeKind, DirHandle, DirState, L1Cache, NodeEntry, NodeSource};
use crate::dispatch::context::RequestContext;
use crate::dispatch::resolve;
use crate::types::file_kind::FileKind;
use crate::types::vfs_path::VfsPath;

impl Router {
    /// Ensure a directory is resolved in the L1 cache.
    ///
    /// If already resolved, this is a no-op. Otherwise calls all active
    /// providers, merges with real filesystem entries, handles conflicts,
    /// allocates inodes, and populates the cache. After all entries are
    /// inserted, stale resolve-sourced entries from prior cycles are
    /// swept — these represent nodes a provider used to emit but no
    /// longer does.
    pub fn ensure_resolved(&self, ctx: &RequestContext<'_>) -> Result<()> {
        // Fast path: already resolved — unless source is stale or dir is volatile.
        if let Some(handle) = self.cache.get(ctx.path) {
            let mut dir = handle.write();
            if dir.is_resolved() {
                if !dir.is_source_stale(|sf| self.file_generations.get(sf))
                    && !is_volatile_in_parent(&self.cache, ctx.path)
                {
                    return Ok(());
                }
                dir.mark_unresolved();
            }
        }

        // Fast path: gitignored / git-internal paths are always passthrough.
        // Skip provider resolution entirely — no provider emits virtual
        // content for these paths.
        if self.path_filter.is_skippable(ctx.path) {
            tracing::trace!(target: "nyne::dispatch", path = %ctx.path, "path filter: skippable → passthrough");
            self.cache.get_or_create(ctx.path).write().mark_passthrough();
            return Ok(());
        }

        // Call providers for virtual entries (handles provider-vs-provider conflicts).
        let virtual_nodes = resolve::resolve_directory(self.registry.active_providers(), ctx)?;

        // Passthrough fast path: no provider emitted virtual nodes.
        // Skip reading real entries, skip inode allocation, skip caching.
        // Real entries will be served directly from RealFs in readdir/lookup.
        if virtual_nodes.is_empty() {
            self.cache.get_or_create(ctx.path).write().mark_passthrough();
            return Ok(());
        }

        // Read real filesystem entries (non-fatal — path may not exist on disk).
        let real_entries = self.real_fs.read_dir(ctx.path).unwrap_or_default();
        let real_names: HashSet<&str> = real_entries.iter().map(|e| e.name.as_str()).collect();

        // Resolve virtual-vs-real conflicts via on_conflict.
        // Providers must Force to shadow a real file; default Yield means real wins.
        let (virtual_nodes, shadowed_names) =
            resolve::resolve_real_conflicts(self.registry.active_providers(), virtual_nodes, &real_names, ctx);

        // Begin a new resolve cycle — bumps generation and marks resolved.
        // insert_node reads the generation from the DirState directly.
        let handle = self.cache.get_or_create(ctx.path);
        let mut dir = handle.write();
        dir.begin_resolve();

        // Insert surviving virtual entries.
        // Derive the directory's source generation from node stamps —
        // all nodes in a companion directory share the same source file.
        let mut dir_source = None;
        for owned in virtual_nodes {
            if dir_source.is_none()
                && let Some((f, generation)) = owned.node.source()
            {
                dir_source = Some((f.clone(), generation));
            }
            let name = owned.node.name().to_owned();
            self.insert_node(&mut dir, ctx.path, NodeEntry {
                name,
                kind: owned.into_cached_kind(),
                source: NodeSource::Children,
            });
        }
        if let Some((source_file, generation)) = dir_source {
            dir.set_source_generation(source_file, generation);
        }

        // Insert real entries not shadowed by a Force-winning provider.
        for entry in real_entries {
            if shadowed_names.contains(&entry.name) {
                continue;
            }
            self.insert_node(&mut dir, ctx.path, NodeEntry {
                name: entry.name,
                kind: CachedNodeKind::Real {
                    file_type: entry.file_type,
                },
                source: NodeSource::Children,
            });
        }

        // Sweep entries from prior resolve cycles that were not refreshed.
        // Lookup-sourced entries are preserved — they persist until
        // explicit invalidation. Evict L2 content for swept inodes.
        let current_gen = dir.resolve_generation();
        let swept_inodes = dir.sweep_stale_resolve(current_gen);
        drop(dir); // Release directory lock before L2 invalidation.
        for inode in swept_inodes {
            self.content_cache.invalidate(inode);
        }

        Ok(())
    }

    /// Look up a specific name in a directory.
    ///
    /// Resolution pipeline: ensure resolved → L1 cache hit → plugin
    /// derivation → provider lookup → real filesystem fallback.
    /// Returns the inode number if found, or `None` if no provider claims it.
    pub fn lookup_name(&self, name: &str, ctx: &RequestContext<'_>) -> Result<Option<u64>> {
        // Step 1: Ensure directory is resolved first.
        self.ensure_resolved(ctx)?;

        // Step 2: Check L1 cache for existing node.
        if let Some(handle) = self.cache.get(ctx.path) {
            let dir = handle.read();
            if let Some(cached) = dir.get(name) {
                return Ok(Some(cached.inode));
            }
        }

        // Step 3: Try plugin derivation — scan sibling nodes for plugins
        // that can derive a node for this name.
        if let Some(handle) = self.cache.get(ctx.path)
            && let Some(derived) = self.try_plugin_derive(&handle, name, ctx)?
        {
            return Ok(Some(derived));
        }

        // Step 4: Not in resolve results — fall back to provider lookup.
        // Only skip for actually skippable paths (gitignored / git-internal).
        // Non-skippable passthrough dirs (no resolve results) still need
        // provider lookup — companion dirs like `file.rs@/` are lookup-only.
        if !self.path_filter.is_skippable(ctx.path) {
            let result = resolve::lookup_name(self.registry.active_providers(), name, ctx)?;

            if let Some(owned) = result {
                let handle = self.cache.get_or_create(ctx.path);
                let mut dir = handle.write();
                let inode = self.insert_node(&mut dir, ctx.path, NodeEntry {
                    name: name.to_owned(),
                    kind: owned.into_cached_kind(),
                    source: NodeSource::Lookup,
                });
                return Ok(Some(inode));
            }
        }

        // Step 5: Real filesystem fallback.
        self.lookup_real(name, ctx)
    }

    /// Look up a name considering only the real filesystem — no providers.
    ///
    /// Used by passthrough processes (git, LSP servers) and as a fallback
    /// from [`lookup_name`] when no provider claims the entry.
    pub(crate) fn lookup_real(&self, name: &str, ctx: &RequestContext<'_>) -> Result<Option<u64>> {
        if let Some(handle) = self.cache.get(ctx.path) {
            let dir = handle.read();
            if let Some(cached) = dir.get(name) {
                // Real entries: reuse the cached inode directly.
                // Virtual entries: a provider Force'd this name. Return the
                // inode only if the real file also exists — the FUSE layer
                // will serve real attrs/content for passthrough processes.
                let is_real =
                    matches!(cached.kind, CachedNodeKind::Real { .. }) || self.real_fs.exists(&ctx.path.join(name)?);
                return Ok(is_real.then_some(cached.inode));
            }
        }
        let child_path = ctx.path.join(name)?;
        if self.real_fs.exists(&child_path) {
            let file_type = if self.real_fs.is_dir(&child_path) {
                FileKind::Directory
            } else {
                self.real_fs
                    .metadata(&child_path)
                    .map(|m| m.file_type)
                    .unwrap_or(FileKind::File)
            };
            let handle = self.cache.get_or_create(ctx.path);
            let mut dir = handle.write();
            let inode = self.insert_node(&mut dir, ctx.path, NodeEntry {
                name: name.to_owned(),
                kind: CachedNodeKind::Real { file_type },
                source: NodeSource::Lookup,
            });
            return Ok(Some(inode));
        }
        Ok(None)
    }

    /// Try plugin derivation by scanning sibling nodes in the directory.
    ///
    /// Takes a write lock: scans entries for plugin-bearing nodes, and if a
    /// plugin produces a derived node, inserts it as [`NodeSource::Derived`].
    /// Returns the inode of the derived node, or `None` if no plugin matched.
    fn try_plugin_derive(&self, handle: &DirHandle, name: &str, ctx: &RequestContext<'_>) -> Result<Option<u64>> {
        let mut dir = handle.write();
        let Some(entry) = derive_from_plugins(&dir, name, ctx)? else {
            return Ok(None);
        };
        Ok(Some(self.insert_node(&mut dir, ctx.path, entry)))
    }

    /// Resolve an inode to an owned snapshot of its data.
    ///
    /// Single lock acquisition: gets the inode entry, finds the cached node,
    /// clones what's needed, and returns an owned snapshot. Returns `None`
    /// for unknown or reserved inodes.
    pub(crate) fn resolve_inode(&self, inode: u64) -> Option<ResolvedInode> {
        let entry = self.inodes.get(inode)?;
        let handle = self.cache.get(&entry.dir_path)?;
        let dir = handle.read();
        let cn = dir.get(&entry.name)?;
        if cn.inode != inode {
            return None; // Stale — replaced by a newer entry.
        }
        match &cn.kind {
            CachedNodeKind::Real { file_type } => {
                let path = entry.dir_path.join(&entry.name).ok()?;
                Some(ResolvedInode::Real {
                    file_type: *file_type,
                    path,
                })
            }
            CachedNodeKind::Virtual { node, provider_id } => Some(ResolvedInode::Virtual {
                node: Arc::clone(node),
                provider_id: *provider_id,
                dir_path: entry.dir_path,
            }),
        }
    }
}

/// Scan directory entries for a plugin that can derive a node for `name`.
///
/// Iterates all cached entries, checking each plugin-bearing virtual node.
/// Returns a ready-to-insert [`NodeEntry`] on first match, `None` otherwise.
fn derive_from_plugins(dir: &DirState, name: &str, ctx: &RequestContext<'_>) -> Result<Option<NodeEntry>> {
    for (_, cn) in dir.all_entries() {
        let CachedNodeKind::Virtual { node, provider_id } = &cn.kind else {
            continue;
        };
        if !node.has_plugins() {
            continue;
        }
        for plugin in node.plugins() {
            let Some(derived) = plugin.derive(node, name, ctx)? else {
                continue;
            };
            return Ok(Some(NodeEntry {
                name: name.to_owned(),
                kind: CachedNodeKind::Virtual {
                    node: Arc::new(derived),
                    provider_id: *provider_id,
                },
                source: NodeSource::Derived,
            }));
        }
    }
    Ok(None)
}

/// Check whether a directory's node in its parent cache is volatile.
///
/// Volatile directories are never considered "resolved" — their contents
/// must be re-resolved on every access because they depend on external
/// state (e.g., LSP workspace search results).
fn is_volatile_in_parent(cache: &L1Cache, path: &VfsPath) -> bool {
    let Some(parent) = path.parent() else { return false };
    let Some(name) = path.name() else { return false };
    let Some(handle) = cache.get(&parent) else { return false };
    let dir = handle.read();
    let Some(entry) = dir.get(name) else { return false };
    matches!(&entry.kind, CachedNodeKind::Virtual { node, .. } if node.is_volatile())
}

#[cfg(test)]
mod tests;
