//! Diff view — unified diff between versions.

use std::str::from_utf8;
use std::sync::Arc;

use color_eyre::eyre::Result;
use nyne::dispatch::context::RequestContext;
use nyne::node::Readable;
use nyne::text::unified_diff;
use nyne::types::vfs_path::VfsPath;

use crate::repo::GitRepo;

/// Target for diff comparison — working tree or a ref.
pub(super) enum DiffTarget {
    /// Working tree vs HEAD — computed without git2 touching the filesystem.
    Workdir { source_file: VfsPath },
    /// HEAD vs an arbitrary ref (branch, tag, sha).
    Ref(String),
}

/// Diff content — produces unified diff on read.
pub(super) struct DiffContent {
    pub repo: Arc<GitRepo>,
    pub rel_path: String,
    pub target: DiffTarget,
}

/// [`Readable`] implementation for [`DiffContent`].
impl Readable for DiffContent {
    /// Reads a unified diff against HEAD or a named ref.
    fn read(&self, ctx: &RequestContext<'_>) -> Result<Vec<u8>> {
        Ok(match &self.target {
            DiffTarget::Workdir { source_file: real_file } => {
                // Read HEAD version from git object store (no filesystem access).
                let old = self.repo.head_blob(&self.rel_path)?;
                let new = ctx.real_fs.read(real_file)?;
                match (from_utf8(&old), from_utf8(&new)) {
                    (Ok(old_str), Ok(new_str)) => unified_diff(old_str, new_str, &self.rel_path).into_bytes(),
                    _ => "Binary file differs\n".into(),
                }
            }
            DiffTarget::Ref(refspec) => self.repo.diff_ref(&self.rel_path, refspec)?.into_bytes(),
        })
    }
}
