use std::path::PathBuf;
use std::sync::Arc;

use color_eyre::eyre::{Result, eyre};
use nyne::router::{AffectedFiles, Filesystem, RenameContext, Renameable};

use crate::repo::Repo;

/// Git-aware file rename: filesystem rename + git index update.
///
/// Attached to companion directory nodes so that `mv old.rs@ new.rs@`
/// performs both the real filesystem rename and a `git mv` equivalent.
pub struct GitFileRename {
    pub(crate) repo: Option<Arc<Repo>>,
    pub(crate) fs: Arc<dyn Filesystem>,
    pub(crate) source_file: PathBuf,
}
/// [`Renameable`] implementation for [`GitFileRename`].
impl Renameable for GitFileRename {
    fn rename(&self, ctx: &RenameContext<'_>) -> Result<AffectedFiles> {
        let new_name = ctx
            .target
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| eyre!("rename target has no filename"))?;

        let new_path = self.source_file.with_file_name(new_name);
        self.fs.rename(&self.source_file, &new_path)?;

        if let Some(repo) = &self.repo {
            let old_rel = repo.rel_path(&self.source_file);
            if repo.is_tracked(&old_rel)? {
                let new_rel = repo.rel_path(&new_path);
                repo.index_rename_with_stat(&old_rel, &new_rel)?;
            }
        }

        Ok(vec![self.source_file.clone(), new_path])
    }
}
