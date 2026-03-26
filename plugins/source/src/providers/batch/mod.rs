//! Batch editing provider — tracks edit operations and staging area.
//!
//! Agents stage edits by writing to virtual files under `edit/` directories
//! (e.g. `file.rs@/symbols/Foo@/edit/replace`). Staged edits accumulate in
//! an in-memory [`StagingMap`] and can be previewed, individually removed,
//! or atomically applied. Application happens when the agent deletes
//! `staged.diff`; clearing (discarding) happens on truncate.
//!
//! The provider contributes to two route namespaces:
//! - **Root `@/edit/`** — cross-file `staged.diff` covering all batches
//! - **Companion `file.rs@/symbols/Foo@/edit/`** — per-symbol staging and preview

/// Anchor resolution for mapping filesystem operations to staged edit actions.
mod anchors;
/// Preview computation for staged edits before application.
mod preview;
/// Staging area state tracking for batch edits.
mod staging;

use std::collections::HashMap;
use std::sync::Arc;

use anchors::resolve_anchor;
use color_eyre::eyre::Result;
use parking_lot::RwLock;
use preview::{CrossFilePreview, StagedActionContent, SymbolPreview};
use staging::{StagedBatch, StagingKey};

/// Shared staging map — `Arc`-wrapped so node capabilities (writers,
/// unlinkers, previews) can hold a reference without lifetime coupling
/// to the provider.
type StagingMap = Arc<RwLock<HashMap<StagingKey, StagedBatch>>>;

use nyne::dispatch::invalidation::InvalidationEvent;
use nyne::dispatch::routing::ctx::RouteCtx;
use nyne::dispatch::routing::tree::RouteTree;
use nyne::node::WriteOutcome;
use nyne::node::builtins::StaticContent;
use nyne::node::capabilities::{Unlinkable, Writable};
use nyne::{companion_symbol_path, dispatch_children, dispatch_lookup, source_file};
use strum::IntoEnumIterator;

use super::names::{FILE_STAGED_DIFF, SUBDIR_EDIT, SUBDIR_STAGED};
use super::prelude::*;
use crate::edit::diff_action::DiffActionNode;
use crate::edit::plan::EditOpKind;
use crate::services::SourceServices;

/// Provider for staging and applying batch edits across symbols and files.
///
/// Exposes two route trees: `@/edit/` for cross-file operations (global
/// `staged.diff`) and per-symbol companion routes (`file.rs@/symbols/Foo@/edit/`)
/// for staging individual edit actions (replace, delete, insert-before,
/// insert-after, append). Staged edits accumulate in memory and are applied
/// atomically when the user deletes `staged.diff`.
pub struct BatchEditProvider {
    ctx: Arc<ActivationContext>,
    /// Per-symbol staged edits.
    batches: StagingMap,
    /// Route tree for `@/edit/...` paths (root companion).
    at_routes: RouteTree<Self>,
    /// Route tree for `file.rs@/symbols/...` companion paths.
    companion_routes: RouteTree<Self>,
}

/// Core construction, route handlers, and node builders.
impl BatchEditProvider {
    /// Create a new batch edit provider with at-routes and companion route trees.
    pub(crate) fn new(ctx: Arc<ActivationContext>) -> Self {
        let at_routes = nyne_macros::routes!(Self, {
            no_emit "@" => children_at_root {
                "edit" => children_root_edit {
                    lookup(lookup_root_edit),
                }
            }
        });

        let companion_routes = nyne_macros::routes!(Self, {
            "symbols" {
                "{..path}@" => children_fragment_scope {
                    "edit" => children_symbol_edit {
                        lookup(lookup_symbol_edit),
                        "staged" => children_staged_dir {
                            lookup(lookup_staged_dir),
                        }
                    }
                }
            }
        });

        Self {
            ctx,
            batches: Arc::new(RwLock::new(HashMap::new())),
            at_routes,
            companion_routes,
        }
    }

    /// Check whether a source file has syntax support (decomposable).
    fn has_syntax_support(&self, source_file: &VfsPath) -> bool {
        source_file
            .extension()
            .is_some_and(|ext| SourceServices::get(&self.ctx).syntax.get(ext).is_some())
    }

    /// Check whether any batches are staged.
    fn has_staged_batches(&self) -> bool { self.batches.read().values().any(|b| !b.is_empty()) }

    /// Build a `StagingKey` from route captures.
    fn staging_key_from_ctx(ctx: &RouteCtx<'_>) -> Result<StagingKey> {
        Ok(StagingKey {
            source_file: source_file(ctx)?,
            fragment_path: ctx.params("path").to_vec(),
        })
    }

    /// `@/` — contribute `edit/` directory if any batches are staged.
    fn children_at_root(&self, _ctx: &RouteCtx<'_>) -> Option<Vec<VirtualNode>> {
        if !self.has_staged_batches() {
            return None;
        }
        Some(vec![VirtualNode::directory(SUBDIR_EDIT)])
    }

    /// `@/edit/` — list `staged.diff`.
    fn children_root_edit(&self, _ctx: &RouteCtx<'_>) -> Vec<VirtualNode> { vec![self.make_global_staged_diff_node()] }

    /// `@/edit/<name>` — lookup within root edit directory.
    fn lookup_root_edit(&self, _ctx: &RouteCtx<'_>, name: &str) -> Option<VirtualNode> {
        if name == FILE_STAGED_DIFF {
            return Some(self.make_global_staged_diff_node());
        }
        None
    }

    /// `file.rs@/symbols/Foo@/` — contribute `edit/` if source has syntax support.
    fn children_fragment_scope(&self, ctx: &RouteCtx<'_>) -> Nodes {
        let source_file = source_file(ctx)?;
        if !self.has_syntax_support(&source_file) {
            return Ok(None);
        }
        Ok(Some(vec![VirtualNode::directory(SUBDIR_EDIT)]))
    }

    /// `file.rs@/symbols/Foo@/edit/` — list edit operations + staged diff + staged dir.
    fn children_symbol_edit(&self, ctx: &RouteCtx<'_>) -> Nodes {
        let key = Self::staging_key_from_ctx(ctx)?;
        let staged_diff = self.make_symbol_staged_diff_node(key.clone());
        let nodes = EditOpKind::iter()
            .map(|kind| self.make_staging_op_node(kind.name(), key.clone(), kind))
            .chain([staged_diff, VirtualNode::directory(SUBDIR_STAGED)])
            .collect();
        Ok(Some(nodes))
    }

    /// `file.rs@/symbols/Foo@/edit/<name>` — lookup within symbol edit directory.
    fn lookup_symbol_edit(&self, ctx: &RouteCtx<'_>, name: &str) -> Node {
        let key = Self::staging_key_from_ctx(ctx)?;
        match name {
            FILE_STAGED_DIFF => Ok(Some(self.make_symbol_staged_diff_node(key))),
            SUBDIR_STAGED => Ok(Some(VirtualNode::directory(SUBDIR_STAGED))),
            _ => Ok(EditOpKind::from_name(name).map(|kind| self.make_staging_op_node(name, key, kind))),
        }
    }

    /// `file.rs@/symbols/Foo@/edit/staged/` — list staged action entries.
    fn children_staged_dir(&self, ctx: &RouteCtx<'_>) -> Nodes {
        let key = Self::staging_key_from_ctx(ctx)?;
        let map = self.batches.read();
        let Some(batch) = map.get(&key) else {
            return Ok(Some(Vec::new()));
        };
        let nodes = batch
            .actions()
            .map(|(index, action)| self.make_staged_action_node(&action.filename(index), key.clone(), index))
            .collect();
        Ok(Some(nodes))
    }

    /// `file.rs@/symbols/Foo@/edit/staged/<name>` — lookup a single staged action.
    fn lookup_staged_dir(&self, ctx: &RouteCtx<'_>, name: &str) -> Node {
        let key = Self::staging_key_from_ctx(ctx)?;
        let Some(index) = parse_staged_filename(name) else {
            return Ok(None);
        };
        let exists = self.batches.read().get(&key).and_then(|b| b.get(index)).is_some();
        if !exists {
            return Ok(None);
        }
        Ok(Some(self.make_staged_action_node(name, key, index)))
    }

    /// Build the cross-file `staged.diff` node.
    fn make_global_staged_diff_node(&self) -> VirtualNode {
        let action = CrossFilePreview {
            batches: Arc::clone(&self.batches),
            ctx: Arc::clone(&self.ctx),
        };
        let clear = StagedDiffClear {
            batches: Arc::clone(&self.batches),
            scope: ClearScope::All,
        };
        VirtualNode::file(FILE_STAGED_DIFF, DiffActionNode::new(FILE_STAGED_DIFF, action.clone()))
            .with_writable(clear)
            .with_unlinkable(DiffActionNode::new(FILE_STAGED_DIFF, action))
            .with_cache_policy(CachePolicy::Never)
    }

    /// Build a per-symbol `staged.diff` node.
    fn make_symbol_staged_diff_node(&self, key: StagingKey) -> VirtualNode {
        let action = SymbolPreview {
            key: key.clone(),
            batches: Arc::clone(&self.batches),
            ctx: Arc::clone(&self.ctx),
        };
        let clear = StagedDiffClear {
            batches: Arc::clone(&self.batches),
            scope: ClearScope::Single(key),
        };
        VirtualNode::file(FILE_STAGED_DIFF, DiffActionNode::new(FILE_STAGED_DIFF, action.clone()))
            .with_writable(clear)
            .with_unlinkable(DiffActionNode::new(FILE_STAGED_DIFF, action))
            .with_cache_policy(CachePolicy::Never)
    }

    /// Build a write-only staging operation node (e.g., `replace`, `delete`).
    fn make_staging_op_node(&self, name: &str, key: StagingKey, kind: EditOpKind) -> VirtualNode {
        let stager = StagingWriter {
            ctx: Arc::clone(&self.ctx),
            batches: Arc::clone(&self.batches),
            key,
            anchor: kind,
        };
        VirtualNode::file(name, StaticContent(b""))
            .with_writable(stager)
            .with_cache_policy(CachePolicy::Never)
    }

    /// Build a read-only staged action entry node (e.g., `10-replace.diff`).
    fn make_staged_action_node(&self, name: &str, key: StagingKey, index: u32) -> VirtualNode {
        let content = StagedActionContent {
            batches: Arc::clone(&self.batches),
            key: key.clone(),
            index,
        };
        let unlinker = StagedActionUnlink {
            batches: Arc::clone(&self.batches),
            key,
            index,
        };
        VirtualNode::file(name, content)
            .with_unlinkable(unlinker)
            .with_cache_policy(CachePolicy::Never)
    }
}

// Staging writer — stages an edit action on write

/// Writable that stages an edit action on write.
///
/// Decomposes the source file at write time to validate the target symbol
/// against fresh fragments — avoids capturing stale decomposition state.
struct StagingWriter {
    ctx: Arc<ActivationContext>,
    batches: StagingMap,
    key: StagingKey,
    anchor: EditOpKind,
}

/// [`Writable`] implementation for [`StagingWriter`].
impl Writable for StagingWriter {
    /// Decompose at write time, resolve the anchor, and stage the resulting action.
    fn write(&self, ctx: &RequestContext<'_>, data: &[u8]) -> Result<WriteOutcome> {
        // Decompose at write time — validates against current source state.
        let parsed = SourceServices::get(&self.ctx)
            .decomposition
            .get(&self.key.source_file)?;

        let action = resolve_anchor(self.anchor, &self.key.fragment_path, data, &parsed.decomposed)?;

        let mut map = self.batches.write();
        let batch = map.entry(self.key.clone()).or_insert_with(StagedBatch::new);
        let _index = batch.stage(action);

        invalidate_symbol_edit_dir(&self.key, ctx);

        Ok(WriteOutcome::Written(data.len()))
    }
}

// Staged action node capabilities (read, write, unlink individual actions)

/// Unlinkable for removing a single staged action.
///
/// Attached to staged action preview nodes (e.g. `10-replace.diff`) so that
/// `rm` on one of those files removes just that action from the batch.
/// If removing the action empties the batch, the entire batch entry is
/// cleaned up.
struct StagedActionUnlink {
    batches: StagingMap,
    key: StagingKey,
    index: u32,
}

/// [`Unlinkable`] implementation for [`StagedActionUnlink`].
impl Unlinkable for StagedActionUnlink {
    /// Remove a single staged action by index, cleaning up the batch if empty.
    fn unlink(&self, ctx: &RequestContext<'_>) -> Result<()> {
        let mut map = self.batches.write();
        let batch = map
            .get_mut(&self.key)
            .ok_or_else(|| color_eyre::eyre::eyre!("no staged edits for {}", self.key.source_file))?;
        batch.remove(self.index);
        // If batch is now empty, remove the entry entirely.
        if batch.is_empty() {
            map.remove(&self.key);
        }
        drop(map);

        invalidate_symbol_edit_dir(&self.key, ctx);
        Ok(())
    }
}

// Truncate writer — clears staged edits when staged.diff is truncated

/// Scope of staged edit clearing.
enum ClearScope {
    /// Clear a single symbol's staged edits.
    Single(StagingKey),
    /// Clear all staged edits across all symbols.
    All,
}

/// Writable for `staged.diff` that clears staged edits on truncate.
///
/// Normal writes are rejected — `staged.diff` is only writable via truncate
/// (the "clear all staged edits" gesture). The `scope` field controls whether
/// a single symbol or all symbols are cleared.
struct StagedDiffClear {
    batches: StagingMap,
    scope: ClearScope,
}

/// [`Writable`] implementation for [`StagedDiffClear`].
impl Writable for StagedDiffClear {
    /// Rejects non-truncate writes with an error message.
    fn write(&self, _ctx: &RequestContext<'_>, _data: &[u8]) -> Result<WriteOutcome> {
        Err(color_eyre::eyre::eyre!(
            "staged.diff is read-only; truncate (write empty) to clear staged edits"
        ))
    }

    /// Clear staged edits on empty truncate write, scoped by [`ClearScope`].
    fn truncate_write(&self, ctx: &RequestContext<'_>, data: &[u8]) -> Result<WriteOutcome> {
        if !data.is_empty() {
            return Err(color_eyre::eyre::eyre!(
                "staged.diff only supports truncate (empty write) to clear staged edits"
            ));
        }
        match &self.scope {
            ClearScope::Single(key) => {
                self.batches.write().remove(key);
                invalidate_symbol_edit_dir(key, ctx);
            }
            ClearScope::All => {
                let mut map = self.batches.write();
                let keys: Vec<StagingKey> = map.keys().cloned().collect();
                map.clear();
                drop(map);
                for key in &keys {
                    invalidate_symbol_edit_dir(key, ctx);
                }
            }
        }
        Ok(WriteOutcome::Written(0))
    }
}

// Cache invalidation

/// Invalidate the symbol's `edit/` directory cache so readdir reflects staging changes.
///
/// Emits a single `Subtree` event for `edit/` — the dispatch layer handles
/// flushing kernel readdir caches for all directories in the subtree via
/// `inval_inode`.
fn invalidate_symbol_edit_dir(key: &StagingKey, ctx: &RequestContext<'_>) {
    // Build path: file.rs@/symbols/Foo@/edit/
    if let Ok(symbol_path) = companion_symbol_path(&key.source_file, &key.fragment_path)
        && let Ok(edit_dir) = symbol_path.join(SUBDIR_EDIT)
    {
        ctx.events.emit(InvalidationEvent::Subtree { path: edit_dir });
    }
}

// Provider trait implementation

/// [`Provider`] implementation for [`BatchEditProvider`].
impl Provider for BatchEditProvider {
    /// Return the batch edit provider identifier.
    fn id(&self) -> ProviderId { Self::PROVIDER_ID }

    /// Dispatch children via both at-routes and companion route trees.
    fn children(self: Arc<Self>, ctx: &RequestContext<'_>) -> Nodes {
        dispatch_children(&self.at_routes, &self.companion_routes, &self, ctx, false)
    }

    /// Dispatch lookup via both at-routes and companion route trees.
    fn lookup(self: Arc<Self>, ctx: &RequestContext<'_>, name: &str) -> Node {
        dispatch_lookup(&self.at_routes, &self.companion_routes, &self, ctx, name, false)
    }
}

/// Parse a staged action filename like `10-replace.diff` → index (10).
fn parse_staged_filename(name: &str) -> Option<u32> {
    let name = name.strip_suffix(".diff")?;
    let (index_str, _label) = name.split_once('-')?;
    index_str.parse().ok()
}

/// Provider ID constant.
impl BatchEditProvider {
    /// Unique provider identifier for batch editing.
    pub(crate) const PROVIDER_ID: ProviderId = ProviderId::new("batch-edit");
}
