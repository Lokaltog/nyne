//! Commit metadata extraction and diff option helpers.
//!
//! Provides [`CommitInfo`] for extracting author, date, hash, and message from
//! `git2::Commit` objects for template rendering. Also provides [`diff_opts`]
//! for creating single-file-scoped `DiffOptions`.

use nyne::text;
use serde::Serialize;

/// Information extracted from a git commit for template rendering.
#[derive(Serialize)]
pub struct CommitInfo {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
    /// Commit timestamp as seconds since epoch (not serialized into templates).
    #[serde(skip)]
    pub epoch_secs: i64,
}
/// Factory methods for [`CommitInfo`].
impl CommitInfo {
    /// Placeholder for uncommitted (working-directory) changes.
    pub(crate) fn uncommitted(oid: git2::Oid) -> Self {
        Self {
            hash: format!("{oid:.7}"),
            author: "uncommitted".into(),
            date: "-".into(),
            message: "uncommitted changes".into(),
            epoch_secs: 0,
        }
    }
}

/// Extract [`CommitInfo`] from a `git2::Commit` for template rendering.
pub fn commit_info(commit: &git2::Commit<'_>) -> CommitInfo {
    let oid = commit.id();
    let epoch_secs = commit.time().seconds();
    CommitInfo {
        hash: format!("{oid:.7}"),
        author: commit.author().name().unwrap_or("unknown").to_owned(),
        date: text::format_git_date(epoch_secs),
        message: commit.message().unwrap_or("").lines().next().unwrap_or("").to_owned(),
        epoch_secs,
    }
}

/// Create `DiffOptions` scoped to a single pathspec.
///
/// Used by blame, log, and diff providers to limit git operations to a
/// single file path rather than the entire repository.
pub fn diff_opts(pathspec: &str) -> git2::DiffOptions {
    let mut opts = git2::DiffOptions::new();
    opts.pathspec(pathspec);
    opts
}
