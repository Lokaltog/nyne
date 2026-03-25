//! Companion directory provider — makes real files/directories accessible with suffix.

use std::sync::Arc;

use color_eyre::eyre::{Result, eyre};
use nyne_macros::routes;

use super::names::companion_name;
use super::prelude::*;
use crate::dispatch::context::RenameContext;
use crate::dispatch::routing::ctx::RouteCtx;
use crate::dispatch::routing::tree::RouteTree;
use crate::node::Renameable;
use crate::types::file_kind::FileKind;

/// Creates companion directories for real files and directories.
///
/// For each real entry in a directory, makes a companion directory accessible
/// with the configured suffix (e.g., `foo.rs@`, `src@`). The companion
/// directory itself is populated by other providers (syntax, git, directory,
/// etc.).
///
/// Companion directories are emitted during resolution with
/// [`Visibility::Hidden`] — hidden from normal readdir listings but
/// surfaced when `ProcessVisibility::All` is active. Renaming a file
/// companion directory (e.g., `mv old.rs@ new.rs@`) renames the
/// underlying real file. Git index updates are handled by
/// plugin-provided companion overlays.
pub(super) struct CompanionProvider {
    routes: RouteTree<Self>,
}

/// Construction and companion node building.
impl CompanionProvider {
    /// Creates a new companion provider with its route tree.
    pub(super) fn new(_ctx: Arc<ActivationContext>) -> Self {
        let routes = routes!(Self, {
            children(children_companions),
            "**" => lookup(lookup_companion),
        });
        Self { routes }
    }

    /// Build a companion node for a real entry.
    ///
    /// File companions get rename capability; directory companions don't.
    /// This is the SSOT for companion node construction — used by both
    /// `children_companions` and `lookup_companion`.
    fn companion_node(dir: &VfsPath, real_name: &str, file_type: FileKind) -> VirtualNode {
        let name = companion_name(real_name);
        let mut node = super::companion_dir(&name);
        if file_type != FileKind::Directory
            && let Ok(source_file) = dir.join(real_name)
        {
            node = node.with_renameable(FileRename { source_file });
        }
        node
    }

    /// Emit hidden companion entries for all real entries in the directory.
    ///
    /// These are `Visibility::Hidden` so they only appear in readdir when
    /// `ProcessVisibility::All` is active.
    #[expect(clippy::unused_self, reason = "route handler called as instance method")]
    fn children_companions(&self, ctx: &RouteCtx<'_>) -> Nodes {
        let nodes: Vec<VirtualNode> = ctx
            .real_fs
            .read_dir(ctx.path)?
            .into_iter()
            .map(|e| Self::companion_node(ctx.path, &e.name, e.file_type))
            .collect();
        Ok(Some(nodes))
    }

    /// Looks up a companion directory for a real file or directory entry.
    #[expect(clippy::unused_self, reason = "route handler called as instance method")]
    fn lookup_companion(&self, ctx: &RouteCtx<'_>, name: &str) -> Node {
        let Some(real_name) = super::strip_companion_suffix(name) else {
            return Ok(None);
        };
        let real_path = ctx.path.join(real_name)?;
        if !ctx.real_fs.exists(&real_path) {
            return Ok(None);
        }
        Ok(Some(Self::companion_node(
            ctx.path,
            real_name,
            if ctx.real_fs.is_dir(&real_path) {
                FileKind::Directory
            } else {
                FileKind::File
            },
        )))
    }
}

/// Provider implementation for companion directories.
impl Provider for CompanionProvider {
    /// Returns the companion provider identifier.
    fn id(&self) -> ProviderId { Self::PROVIDER_ID }

    /// Dispatches children resolution through the route tree.
    fn children(self: Arc<Self>, ctx: &RequestContext<'_>) -> Nodes { self.routes.children(&self, ctx) }

    /// Dispatches lookup resolution through the route tree.
    fn lookup(self: Arc<Self>, ctx: &RequestContext<'_>, name: &str) -> Node { self.routes.lookup(&self, ctx, name) }
}

/// Provider identifier constant.
impl CompanionProvider {
    /// The provider identifier for the companion provider.
    pub(super) const PROVIDER_ID: ProviderId = ProviderId::new("at-companion");
}

/// Rename capability for companion directory nodes.
///
/// Renames the real file on disk. Git index updates (if needed) are
/// the responsibility of the git plugin — it can register its own
/// companion provider with `ConflictResolution::Force` to wrap renames.
struct FileRename {
    source_file: VfsPath,
}

/// Rename implementation that renames the underlying real file.
impl Renameable for FileRename {
    /// Renames the real file, stripping the companion suffix from the target name.
    fn rename(&self, ctx: &RenameContext<'_>) -> Result<()> {
        let new_name = super::strip_companion_suffix(ctx.target_name)
            .ok_or_else(|| eyre!("rename target must end with companion suffix (@)"))?;

        let parent = self.source_file.parent().unwrap_or(VfsPath::root());
        let new_path = parent.join(new_name)?;

        ctx.request.real_fs.rename(&self.source_file, &new_path)
    }
}
