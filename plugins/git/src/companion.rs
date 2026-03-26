//! Git-aware companion overlay provider.
//!
//! Overlays core's `CompanionProvider` with `ConflictResolution::Force` to add
//! git index updates on file renames (`mv old.rs@ new.rs@` → `git mv`).

use color_eyre::eyre::eyre;
use nyne::companion_dir;
use nyne::dispatch::context::RenameContext;
use nyne::node::Renameable;
use nyne::prelude::*;
use nyne::provider::{ConflictInfo, ConflictResolution};
use nyne::types::path_conventions::strip_companion_suffix;

use crate::repo::GitRepo;

/// Companion overlay that adds git-aware renames to file companion directories.
///
/// Registers on the same routes as core's `CompanionProvider` and wins via
/// `ConflictResolution::Force`. Only activates when a git repo is present.
pub struct GitCompanionProvider {
    ctx: Arc<ActivationContext>,
}

/// Associated constants and constructor for [`GitCompanionProvider`].
impl GitCompanionProvider {
    /// Provider identifier for the git companion.
    pub(crate) const PROVIDER_ID: ProviderId = ProviderId::new("git-companion");

    /// Creates a new git companion provider.
    pub(crate) const fn new(ctx: Arc<ActivationContext>) -> Self { Self { ctx } }
}

/// [`Provider`] implementation for [`GitCompanionProvider`].
impl Provider for GitCompanionProvider {
    /// Returns the provider identifier.
    fn id(&self) -> ProviderId { Self::PROVIDER_ID }

    /// Returns child nodes (none for companion).
    fn children(self: Arc<Self>, _ctx: &RequestContext<'_>) -> Nodes { Ok(None) }

    /// Wins conflicts over the core companion provider.
    fn on_conflict(
        self: Arc<Self>,
        _ctx: &RequestContext<'_>,
        _conflicts: &[ConflictInfo],
    ) -> Result<ConflictResolution> {
        // Win over core CompanionProvider — our rename does `git mv`.
        Ok(ConflictResolution::Force(vec![]))
    }

    /// Looks up a companion node for a real file, attaching git-aware rename.
    fn lookup(self: Arc<Self>, ctx: &RequestContext<'_>, name: &str) -> Node {
        // Only handle companion paths for real files (not directories).
        let Some(real_name) = strip_companion_suffix(name) else {
            return Ok(None);
        };
        let real_path = ctx.path.join(real_name)?;
        if !ctx.real_fs.exists(&real_path) || ctx.real_fs.is_dir(&real_path) {
            return Ok(None);
        }

        let repo = self.ctx.get::<Arc<GitRepo>>();
        let node = companion_dir(name).with_renameable(GitFileRename {
            repo: repo.cloned(),
            source_file: real_path,
        });

        Ok(Some(node))
    }
}

/// Git-aware file rename: filesystem rename + git index update.
///
/// Attached to companion directory nodes so that `mv old.rs@ new.rs@`
/// performs both the real filesystem rename and a `git mv` equivalent
/// (removing the old path and adding the new path in the git index).
struct GitFileRename {
    repo: Option<Arc<GitRepo>>,
    source_file: VfsPath,
}

/// [`Renameable`] implementation for [`GitFileRename`].
impl Renameable for GitFileRename {
    /// Renames the source file on disk and updates the git index.
    fn rename(&self, ctx: &RenameContext<'_>) -> Result<()> {
        let new_name = strip_companion_suffix(ctx.target_name)
            .ok_or_else(|| eyre!("rename target must end with companion suffix (@)"))?;

        let parent = self.source_file.parent().unwrap_or(VfsPath::root());
        let new_path = parent.join(new_name)?;

        // Rename the real file on disk.
        ctx.request.real_fs.rename(&self.source_file, &new_path)?;

        // Update the git index if the file was tracked.
        if let Some(repo) = &self.repo {
            let old_rel = repo.rel_path(&self.source_file);
            if repo.is_tracked(&old_rel)? {
                let new_rel = repo.rel_path(&new_path);
                repo.index_rename_with_stat(&old_rel, &new_rel)?;
            }
        }

        Ok(())
    }
}
