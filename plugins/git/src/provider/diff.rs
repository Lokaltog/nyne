//! Diff view — unified diff between versions.

use std::path::PathBuf;
use std::str::from_utf8;
use std::sync::Arc;

use color_eyre::eyre::Result;
use nyne::router::{ReadContext, Readable};
use nyne::text::unified_diff;

use crate::repo::Repo;

/// Target for diff comparison — working tree or a named ref.
///
/// `Workdir` reads the overlay file directly and diffs against HEAD's blob,
/// avoiding git2 filesystem access. `Ref` diffs HEAD against an arbitrary
/// branch, tag, or commit SHA.
pub(super) enum DiffTarget {
    /// Working tree vs HEAD — computed without git2 touching the filesystem.
    Workdir { source_file: PathBuf },
    /// HEAD vs an arbitrary ref (branch, tag, sha).
    Ref(String),
}

/// Diff content — produces unified diff on read.
pub(super) struct DiffContent {
    pub repo: Arc<Repo>,
    pub rel_path: String,
    pub target: DiffTarget,
}

/// [`Readable`] implementation for [`DiffContent`].
impl Readable for DiffContent {
    /// Reads a unified diff against HEAD or a named ref.
    fn read(&self, ctx: &ReadContext<'_>) -> Result<Vec<u8>> {
        Ok(match &self.target {
            DiffTarget::Workdir { source_file: real_file } => {
                // Read HEAD version from git object store (no filesystem access).
                let old = self.repo.head_blob(&self.rel_path)?;
                let new = ctx.fs.read_file(real_file)?;
                match (from_utf8(&old), from_utf8(&new)) {
                    (Ok(old_str), Ok(new_str)) => diff_or_sentinel(unified_diff(old_str, new_str, &self.rel_path)),
                    _ => "Binary file differs\n".into(),
                }
            }
            DiffTarget::Ref(refspec) => diff_or_sentinel(self.repo.diff_ref(&self.rel_path, refspec)?),
        })
    }
}

/// Convert diff text to bytes, substituting a sentinel when there are no changes.
///
/// Both `unified_diff` and `Repo::diff_ref` return an empty string when the
/// inputs are identical; render an explicit `"No changes\n"` placeholder so
/// readers see actionable output instead of a zero-byte file.
fn diff_or_sentinel(diff: String) -> Vec<u8> {
    if diff.is_empty() {
        b"No changes\n".to_vec()
    } else {
        diff.into_bytes()
    }
}
