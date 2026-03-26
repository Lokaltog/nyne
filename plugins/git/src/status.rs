//! Repository status: branch, tracking, dirty state, recent commits.

use color_eyre::eyre::Result;
use tracing::warn;

use crate::commit::{CommitInfo, commit_info};
use crate::repo::GitRepo;

/// Full repository status snapshot for template rendering.
///
/// Captures branch, tracking, stash, recent commits, and per-file status
/// in a single pass via `GitRepo::status()`. Serialized into the
/// `STATUS.md` Jinja template.
#[derive(serde::Serialize)]
pub struct RepoStatus {
    pub branch: String,
    pub tracking: Option<TrackingInfo>,
    pub stash_count: usize,
    pub recent_commits: Vec<CommitInfo>,
    pub staged: Vec<StatusEntry>,
    pub modified: Vec<StatusEntry>,
    pub untracked: Vec<String>,
    pub conflicted: Vec<String>,
}

/// Upstream tracking relationship.
#[derive(serde::Serialize)]
pub struct TrackingInfo {
    pub(crate) remote: String,
    pub(crate) ahead: usize,
    pub(crate) behind: usize,
}

/// A single file with a status label (e.g. "modified", "deleted").
#[derive(serde::Serialize)]
pub struct StatusEntry {
    pub(crate) path: String,
    pub(crate) label: &'static str,
}

/// Status-related methods on [`GitRepo`].
impl GitRepo {
    /// Snapshot the full repository status.
    pub(crate) fn status(&self) -> Result<RepoStatus> {
        let branch = self.head_branch();
        let repo = self.lock();

        let tracking = tracking_info(&repo);
        let stash_count = stash_count(&repo);
        let recent_commits = recent_commits(&repo, RECENT_COMMITS_LIMIT).unwrap_or_else(|e| {
            warn!(error = %e, "failed to collect recent commits");
            Vec::new()
        });

        // Full status: HEAD <-> index <-> workdir. Safe because the repo was
        // opened on the pre-mount real path — git2 stats the real filesystem
        // directly, never through FUSE.
        let mut opts = git2::StatusOptions::new();
        opts.show(git2::StatusShow::IndexAndWorkdir)
            .include_untracked(true)
            .recurse_untracked_dirs(true)
            .include_ignored(false);
        let statuses = repo.statuses(Some(&mut opts))?;

        let mut staged = Vec::new();
        let mut modified = Vec::new();
        let mut untracked = Vec::new();
        let mut conflicted = Vec::new();

        for entry in statuses.iter() {
            let path = entry.path().unwrap_or("?");
            let s = entry.status();

            if s.is_conflicted() {
                conflicted.push(path.to_owned());
            }
            if let Some(label) = classify_status(s, INDEX_LABELS) {
                staged.push(StatusEntry {
                    path: path.to_owned(),
                    label,
                });
            }
            if let Some(label) = classify_status(s, WORKDIR_LABELS) {
                modified.push(StatusEntry {
                    path: path.to_owned(),
                    label,
                });
            }
            if s.is_wt_new() {
                untracked.push(path.to_owned());
            }
        }

        Ok(RepoStatus {
            branch,
            tracking,
            stash_count,
            recent_commits,
            staged,
            modified,
            untracked,
            conflicted,
        })
    }
}

/// Return the first matching label from `table` for a given git status.
fn classify_status(s: git2::Status, table: &[(git2::Status, &'static str)]) -> Option<&'static str> {
    table
        .iter()
        .find_map(|&(flag, label)| s.contains(flag).then_some(label))
}

/// Status flag to label mapping for staged (index) changes.
const INDEX_LABELS: &[(git2::Status, &str)] = &[
    (git2::Status::INDEX_NEW, "new file"),
    (git2::Status::INDEX_MODIFIED, "modified"),
    (git2::Status::INDEX_DELETED, "deleted"),
    (git2::Status::INDEX_RENAMED, "renamed"),
    (git2::Status::INDEX_TYPECHANGE, "typechange"),
];

/// Status flag to label mapping for unstaged (working directory) changes.
const WORKDIR_LABELS: &[(git2::Status, &str)] = &[
    (git2::Status::WT_MODIFIED, "modified"),
    (git2::Status::WT_DELETED, "deleted"),
    (git2::Status::WT_RENAMED, "renamed"),
    (git2::Status::WT_TYPECHANGE, "typechange"),
];

/// Number of recent commits to show in status.
const RECENT_COMMITS_LIMIT: usize = 10;

/// Resolve upstream tracking info for the current branch.
///
/// Returns `None` on any failure (detached HEAD, no upstream, etc.) — tracking
/// info is best-effort, never an error.
fn tracking_info(repo: &git2::Repository) -> Option<TrackingInfo> {
    let head = repo.head().ok()?;
    let branch_name = head.name()?;
    let upstream_buf = repo.branch_upstream_name(branch_name).ok()?;
    let upstream_name = upstream_buf.as_str()?;
    let upstream_ref = repo.find_reference(upstream_name).ok()?;
    let local_oid = head.target()?;
    let upstream_oid = upstream_ref.target()?;
    let (ahead, behind) = repo.graph_ahead_behind(local_oid, upstream_oid).ok()?;
    let display = upstream_name
        .strip_prefix("refs/remotes/")
        .unwrap_or(upstream_name)
        .to_owned();
    Some(TrackingInfo {
        remote: display,
        ahead,
        behind,
    })
}

/// Count stash entries. Best-effort — returns 0 on any error.
fn stash_count(repo: &git2::Repository) -> usize { repo.reflog("refs/stash").map(|log| log.len()).unwrap_or(0) }

/// Collect the N most recent commits on HEAD.
fn recent_commits(repo: &git2::Repository, limit: usize) -> Result<Vec<CommitInfo>> {
    let mut revwalk = repo.revwalk()?;
    revwalk.set_sorting(git2::Sort::TIME)?;
    revwalk.push_head()?;
    let mut commits = Vec::new();
    for oid_result in revwalk.take(limit) {
        let oid = oid_result?;
        let commit = repo.find_commit(oid)?;
        commits.push(commit_info(&commit, oid));
    }
    Ok(commits)
}
