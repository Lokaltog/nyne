//! Commit information and diff options helpers.

use git2::Oid;
use nyne::format;
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

/// Extract [`CommitInfo`] from a `git2::Commit` for template rendering.
pub fn commit_info(commit: &git2::Commit<'_>, oid: Oid) -> CommitInfo {
    let epoch_secs = commit.time().seconds();
    CommitInfo {
        hash: format!("{oid:.7}"),
        author: commit.author().name().unwrap_or("unknown").to_owned(),
        date: format::format_git_date(epoch_secs),
        message: commit.message().unwrap_or("").lines().next().unwrap_or("").to_owned(),
        epoch_secs,
    }
}

/// Create `DiffOptions` scoped to a single pathspec.
pub fn diff_opts(pathspec: &str) -> git2::DiffOptions {
    let mut opts = git2::DiffOptions::new();
    opts.pathspec(pathspec);
    opts
}
