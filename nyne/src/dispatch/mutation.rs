//! File system mutation operations: create, remove, rename, with provider delegation.

use std::io::{Error as IoError, ErrorKind};
use std::sync::Arc;

use color_eyre::eyre::{Report, Result, ensure};

use super::cache::{CachedNodeKind, NodeEntry, NodeSource};
use super::resolve::{self, OwnedNode};
use super::router::{ResolvedInode, Router};
use crate::dispatch::context::{RenameContext, RequestContext};
use crate::provider::mutation::MutationOp;
use crate::provider::{MutationOutcome, Provider, ProviderId};
use crate::types::file_kind::FileKind;
use crate::types::vfs_path::VfsPath;

/// Create an eyre error that embeds an [`io::Error`] for FUSE errno extraction.
///
/// The FUSE layer's [`extract_errno`](crate::fuse::extract_errno) walks the
/// eyre chain to find an `io::Error` and maps its `ErrorKind` to a FUSE
/// errno. Without this, all dispatch errors surface as `EIO`.
pub(super) fn io_err(kind: ErrorKind, msg: impl Into<String>) -> Report { IoError::new(kind, msg.into()).into() }

/// `AlreadyExists` — entry with `name` already present at `path`.
fn entry_exists(name: &str, path: &VfsPath) -> Report {
    io_err(
        ErrorKind::AlreadyExists,
        format!("entry \"{name}\" already exists at {path}"),
    )
}

/// `NotFound` — inode could not be resolved back to a node.
fn inode_not_found(inode: u64, name: &str) -> Report {
    io_err(
        ErrorKind::NotFound,
        format!("failed to resolve inode {inode} for \"{name}\""),
    )
}

/// `NotFound` — named entry missing from its parent directory.
fn entry_not_found(name: &str, path: &VfsPath) -> Report {
    io_err(ErrorKind::NotFound, format!("entry \"{name}\" not found at {path}"))
}

/// `NotFound` — directory itself is missing from L1 cache.
fn dir_not_found(path: &VfsPath) -> Report { io_err(ErrorKind::NotFound, format!("directory not found: {path}")) }

/// Mutation dispatch methods: create, remove, rename, and real-FS fallback.
impl Router {
    /// Create a new file node in a directory.
    ///
    /// First queries all active providers with single-claim semantics.
    /// If no provider claims the name and the parent directory exists on
    /// the real filesystem, falls back to creating a real file via
    /// `dispatch_real_mutation`.
    /// Returns `None` if neither providers nor the real FS can handle it
    /// (e.g., virtual-only directories like `@/git/`).
    pub fn create_node(&self, name: &str, ctx: &RequestContext<'_>) -> Result<Option<u64>> {
        self.create_or_mkdir(
            name,
            ctx,
            resolve::create_in_directory,
            |p| MutationOp::Create { path: p },
            FileKind::File,
        )
    }

    /// Create a new directory node.
    ///
    /// First queries all active providers with single-claim semantics.
    /// If no provider claims the name and the parent directory exists on
    /// the real filesystem, falls back to creating a real directory via
    /// `dispatch_real_mutation`.
    /// Returns `None` if neither providers nor the real FS can handle it.
    pub fn mkdir_node(&self, name: &str, ctx: &RequestContext<'_>) -> Result<Option<u64>> {
        self.create_or_mkdir(
            name,
            ctx,
            resolve::mkdir_in_directory,
            |p| MutationOp::Mkdir { path: p },
            FileKind::Directory,
        )
    }

    /// Unified create/mkdir implementation.
    ///
    /// Delegates to providers via `provision_node`, then falls back to
    /// real-FS mutation if no provider claims the name and the parent
    /// directory exists on disk. Drains provider events before returning
    /// so callers don't need to call `process_events()` separately.
    fn create_or_mkdir(
        &self,
        name: &str,
        ctx: &RequestContext<'_>,
        resolve_fn: impl FnOnce(&[Arc<dyn Provider>], &str, &RequestContext<'_>) -> Result<Option<OwnedNode>>,
        make_op: impl FnOnce(&VfsPath) -> MutationOp<'_>,
        file_kind: FileKind,
    ) -> Result<Option<u64>> {
        // Phase 1: provider delegation — a provider may claim this name
        // (e.g., creating a file inside a virtual directory).
        if let Some(inode) = self.provision_node(name, ctx, resolve_fn)? {
            self.process_events();
            return Ok(Some(inode));
        }

        // Phase 2: real-FS fallback — only if the parent exists on disk.
        // Virtual-only directories (e.g., `@/git/`) have no real parent,
        // so creation correctly returns None.
        if !self.real_fs.is_dir(ctx.path) {
            return Ok(None);
        }
        let path = ctx.path.join(name)?;
        self.dispatch_real_mutation(&make_op(&path))?;
        let inode = self.insert_real_entry(name, file_kind, ctx.path);
        self.process_events();
        Ok(Some(inode))
    }

    /// Provision a new node via provider delegation (create or mkdir).
    ///
    /// Ensures directory is resolved, checks name doesn't exist,
    /// delegates to the given resolve function, and inserts the result
    /// into L1 cache with `NodeSource::Mutated`.
    fn provision_node(
        &self,
        name: &str,
        ctx: &RequestContext<'_>,
        resolve_fn: impl FnOnce(&[Arc<dyn Provider>], &str, &RequestContext<'_>) -> Result<Option<OwnedNode>>,
    ) -> Result<Option<u64>> {
        self.ensure_resolved(ctx)?;

        if let Some(handle) = self.cache.get(ctx.path) {
            let dir = handle.read();
            if dir.get(name).is_some() {
                return Err(entry_exists(name, ctx.path));
            }
        }

        let result = resolve_fn(self.registry.active_providers(), name, ctx)?;

        let Some(owned) = result else {
            return Ok(None);
        };

        let handle = self.cache.get_or_create(ctx.path);
        let mut dir = handle.write();
        let inode = self.insert_node(&mut dir, ctx.path, NodeEntry {
            name: name.to_owned(),
            kind: owned.into_cached_kind(),
            source: NodeSource::Mutated,
        });

        Ok(Some(inode))
    }

    /// Remove a node from a directory (virtual or real file).
    ///
    /// **Virtual nodes:** resolves the name, checks for the `Unlinkable`
    /// capability, and calls `unlink()`. L2 content cache is invalidated.
    ///
    /// **Real files:** dispatched through `dispatch_real_mutation`
    /// — providers can intercept the operation (e.g., `git rm`),
    /// otherwise the router falls back to `RealFs::unlink()` /
    /// `RealFs::rmdir()`.
    ///
    /// L1 cache eviction and event processing run unconditionally after
    /// the type-specific operation, for both virtual and real nodes.
    pub fn remove_node(&self, name: &str, is_dir: bool, ctx: &RequestContext<'_>) -> Result<()> {
        self.ensure_resolved(ctx)?;

        let inode = self.lookup_inode_in_dir(name, ctx)?;

        let Some(resolved) = self.resolve_inode(inode) else {
            return Err(inode_not_found(inode, name));
        };

        match resolved {
            ResolvedInode::Virtual { node, dir_path, .. } => {
                let unlinkable = node.require_unlinkable()?;

                let unlink_ctx = self.make_request_context(&dir_path);
                unlinkable.unlink(&unlink_ctx)?;

                self.content_cache.invalidate(inode);
                // Bump file generation so sibling companion entries
                // (OVERVIEW, body, docstring, etc.) are lazily detected
                // as stale on next access — same as write_content does.
                if let Some((source_file, _)) = node.source() {
                    self.file_generations.bump(source_file);
                }
            }
            ResolvedInode::Real { path, .. } => {
                let op = if is_dir {
                    MutationOp::Rmdir { path: &path }
                } else {
                    MutationOp::Unlink { path: &path }
                };
                self.dispatch_real_mutation(&op)?;
            }
        }

        // Inline L1 eviction: remove the entry immediately so subsequent
        // lookup/create calls see the current state. Without this, fast
        // create→remove→create cycles race against the async watcher —
        // especially for skippable paths (.git/*) where the watcher skips
        // cache invalidation entirely.
        {
            let handle = self.cache.get_or_create(ctx.path);
            handle.write().remove(name);
        }
        self.process_events();

        Ok(())
    }

    /// Rename a node (virtual or real file).
    ///
    /// **Virtual nodes:** resolves the source, checks for the `Renameable`
    /// capability, and delegates to `Renameable::rename()`. L2 content
    /// cache is invalidated for the source inode.
    ///
    /// **Real files:** dispatched through `dispatch_real_mutation`
    /// — providers can intercept the operation, otherwise the router
    /// falls back to `RealFs::rename()`. The L1 cache entry is moved
    /// (not evicted) so the inode remains resolvable immediately after
    /// the rename — this is critical for editor atomic-save patterns
    /// (rename → create → unlink) where the kernel reuses the inode
    /// number from the rename reply.
    ///
    /// Both paths invalidate affected directories and process events.
    pub fn rename_node(
        &self,
        src_name: &str,
        src_ctx: &RequestContext<'_>,
        target_dir: &VfsPath,
        target_name: &str,
    ) -> Result<()> {
        self.ensure_resolved(src_ctx)?;

        let inode = self.lookup_inode_in_dir(src_name, src_ctx)?;

        let Some(resolved) = self.resolve_inode(inode) else {
            return Err(inode_not_found(inode, src_name));
        };

        let target_path = target_dir.join(target_name)?;

        match resolved {
            ResolvedInode::Virtual { node, dir_path, .. } => {
                let renameable = node.require_renameable()?;
                let ctx = self.make_request_context(&dir_path);
                let rename_ctx = RenameContext {
                    request: &ctx,
                    target_path: &target_path,
                    target_name,
                };
                renameable.rename(&rename_ctx)?;

                self.content_cache.invalidate(inode);

                // Virtual renames: evict old entry + invalidate target.
                {
                    let handle = self.cache.get_or_create(src_ctx.path);
                    handle.write().remove(src_name);
                }
                self.cache.invalidate_dir(target_dir);
            }
            ResolvedInode::Real { path, .. } => {
                self.dispatch_real_mutation(&MutationOp::Rename {
                    from: &path,
                    to: &target_path,
                })?;
                self.update_cache_after_real_rename(inode, src_name, src_ctx.path, target_dir, target_name);
            }
        }

        self.process_events();
        Ok(())
    }

    /// Move a cache entry across directories during a cross-dir rename.
    fn rename_cross_dir(&self, inode: u64, src_name: &str, src_dir: &VfsPath, target_dir: &VfsPath, target_name: &str) {
        let cached_node = {
            let handle = self.cache.get_or_create(src_dir);
            handle.write().remove_entry(src_name)
        };
        if let Some(node) = cached_node {
            let handle = self.cache.get_or_create(target_dir);
            handle.write().insert(target_name.to_owned(), node);
        }
        let target_parent = self.parent_inode_for_dir(target_dir);
        self.inodes
            .update(inode, target_dir.clone(), target_name.to_owned(), target_parent);
    }

    /// Update L1 cache and inode map after a real-file rename.
    ///
    /// Moves the cache entry (same-dir) or delegates to
    /// [`rename_cross_dir`](Self::rename_cross_dir) (cross-dir), then
    /// invalidates both directories for provider re-resolution.
    fn update_cache_after_real_rename(
        &self,
        inode: u64,
        src_name: &str,
        src_dir: &VfsPath,
        target_dir: &VfsPath,
        target_name: &str,
    ) {
        if src_dir == target_dir {
            let handle = self.cache.get_or_create(src_dir);
            handle.write().move_entry(src_name, target_name.to_owned());
            self.inodes.update(
                inode,
                src_dir.clone(),
                target_name.to_owned(),
                self.parent_inode_for_dir(src_dir),
            );
        } else {
            self.rename_cross_dir(inode, src_name, src_dir, target_dir, target_name);
        }
        self.cache.invalidate_dir(src_dir);
        self.cache.invalidate_dir(target_dir);
    }

    /// Dispatch a real-file mutation through the provider intercept chain.
    ///
    /// Uses single-claim semantics: all providers are queried, and at
    /// most one may return `Handled`. If multiple providers claim the
    /// same mutation, the operation fails (ambiguous). If none claim it,
    /// the router falls back to the corresponding [`RealFs`] method.
    ///
    /// Either way, the actual filesystem change triggers inotify events
    /// that flow through the watcher for cache invalidation (SSOT).
    fn dispatch_real_mutation(&self, op: &MutationOp<'_>) -> Result<()> {
        // Collect providers that claim this mutation.
        let handlers: Vec<ProviderId> = self
            .registry
            .active_providers()
            .iter()
            .filter_map(|provider| match provider.handle_mutation(op, self.real_fs.as_ref()) {
                Ok(MutationOutcome::Handled) => Some(provider.id()),
                Ok(MutationOutcome::NotHandled) => None,
                Err(e) => {
                    tracing::warn!(
                        provider = %provider.id(),
                        ?op,
                        error = %e,
                        "provider handle_mutation failed"
                    );
                    None
                }
            })
            .collect();

        ensure!(
            handlers.len() <= 1,
            "ambiguous mutation: providers [{}] all claimed {op:?}",
            handlers.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
        );

        // No provider claimed — fall back to direct filesystem operation.
        if handlers.is_empty() {
            match op {
                MutationOp::Rename { from, to } => self.real_fs.rename(from, to)?,
                MutationOp::Unlink { path } => self.real_fs.unlink(path)?,
                MutationOp::Rmdir { path } => self.real_fs.rmdir(path)?,
                MutationOp::Create { path } => self.real_fs.create_file(path)?,
                MutationOp::Mkdir { path } => self.real_fs.mkdir(path)?,
            }
        }
        Ok(())
    }

    /// Insert a real filesystem entry into L1 cache after a mutation.
    ///
    /// Used after `dispatch_real_mutation` creates a file/directory on
    /// the real filesystem — populates the cache immediately so the
    /// FUSE reply can return valid inode/attrs without waiting for
    /// watcher back-propagation.
    fn insert_real_entry(&self, name: &str, file_type: FileKind, dir_path: &VfsPath) -> u64 {
        let handle = self.cache.get_or_create(dir_path);
        let mut dir = handle.write();
        self.insert_node(&mut dir, dir_path, NodeEntry {
            name: name.to_owned(),
            kind: CachedNodeKind::Real { file_type },
            source: NodeSource::Mutated,
        })
    }

    /// Look up the inode for a named entry in a directory's L1 cache.
    ///
    /// Returns a semantic `NotFound` error if the directory or entry
    /// is missing — used by `remove_node` and `rename_node` to avoid
    /// duplicating the same lookup + error construction.
    fn lookup_inode_in_dir(&self, name: &str, ctx: &RequestContext<'_>) -> Result<u64> {
        let handle = self.cache.get(ctx.path).ok_or_else(|| dir_not_found(ctx.path))?;
        let dir = handle.read();
        let inode = dir.get(name).ok_or_else(|| entry_not_found(name, ctx.path))?.inode;
        Ok(inode)
    }
}
