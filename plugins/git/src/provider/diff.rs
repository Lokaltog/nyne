//! Diff view — unified diff between versions.

use std::str::from_utf8;
use std::sync::Arc;

use color_eyre::eyre::Result;
use nyne::dispatch::context::RequestContext;
use nyne::format::unified_diff;
use nyne::node::Readable;
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
        let text = match &self.target {
            DiffTarget::Workdir { source_file: real_file } => {
                // Read HEAD version from git object store (no filesystem access).
                let old = self.repo.head_blob(&self.rel_path)?;
                let new = ctx.real_fs.read(real_file)?;
                unified_diff(
                    from_utf8(&old).unwrap_or(""),
                    from_utf8(&new).unwrap_or(""),
                    &self.rel_path,
                )
            }
            DiffTarget::Ref(refspec) => self.repo.diff_ref(&self.rel_path, refspec)?,
        };
        Ok(text.into_bytes())
    }
}
