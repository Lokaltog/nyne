//! History data types and git querying — blame hunks, commits, contributors, notes.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use color_eyre::eyre::{Result, WrapErr};
use git2::Oid;
use nyne::dispatch::context::RequestContext;
use nyne::node::Readable;
use nyne::types::SymbolLineRange;

use crate::repo::GitRepo;
use crate::{CommitInfo, commit_info, diff_opts};

/// Safety cap on revwalk iterations to prevent unbounded history walks.
const MAX_REVWALK: usize = 5000;

/// Blame hunk with line range and commit metadata.
#[derive(serde::Serialize)]
pub struct BlameHunk {
    pub lines: String,
    #[serde(skip)]
    pub start_line: usize,
    #[serde(skip)]
    pub end_line: usize,
    #[serde(flatten)]
    pub commit: CommitInfo,
}

/// A single history entry (commit that touched a file).
#[derive(serde::Serialize)]
pub struct HistoryEntry {
    #[serde(skip)]
    pub oid: Oid,
    #[serde(flatten)]
    pub commit: CommitInfo,
}

/// An author with commit count.
#[derive(serde::Serialize)]
pub(super) struct Contributor {
    pub name: String,
    pub commits: usize,
}

/// A git note attached to a commit.
#[derive(serde::Serialize)]
pub(super) struct NoteEntry {
    pub hash: String,
    pub date: String,
    pub commit_message: String,
    pub note: String,
}

/// Returns the raw file content at a specific commit.
pub struct HistoryVersionContent {
    pub repo: Arc<GitRepo>,
    pub rel_path: String,
    pub oid: Oid,
}

impl Readable for HistoryVersionContent {
    fn read(&self, _ctx: &RequestContext<'_>) -> Result<Vec<u8>> { self.repo.blob_at(&self.rel_path, self.oid) }
}

impl GitRepo {
    /// Collect blame hunks for a file, including uncommitted changes.
    ///
    /// Safe to call during FUSE callbacks — the repo is opened via
    /// the overlay merged path, so workdir reads resolve against the overlay
    /// (not through FUSE).
    pub fn blame(&self, rel_path: &str) -> Result<Vec<BlameHunk>> {
        let repo = self.lock();

        let blame = repo
            .blame_file(Path::new(rel_path), None)
            .wrap_err("git blame failed")?;

        (0..blame.len())
            .filter_map(|i| blame.get_index(i))
            .map(|hunk| blame_hunk(&repo, &hunk))
            .collect()
    }

    /// Commits that touched `rel_path`, newest first, capped at `limit`.
    pub(super) fn file_history(&self, rel_path: &str, limit: usize) -> Result<Vec<HistoryEntry>> {
        let repo = self.lock();
        walk_file_commits(&repo, rel_path, limit, commit_entry)
    }

    pub fn file_history_in_range(
        &self,
        rel_path: &str,
        range: &SymbolLineRange,
        limit: usize,
    ) -> Result<Vec<HistoryEntry>> {
        let repo = self.lock();
        // Cap at the same safety limit as full history.
        let all = walk_file_commits(&repo, rel_path, super::log::LOG_LIMIT, commit_entry)?;
        let filtered: Vec<_> = all
            .into_iter()
            .filter(|entry| commit_touches_range(&repo, entry.oid, rel_path, range).unwrap_or(false))
            .take(limit)
            .collect();
        Ok(filtered)
    }

    /// Most recent commit that touched `rel_path`, as seconds since epoch.
    ///
    /// Falls back to HEAD time if the revwalk finds nothing.
    pub fn file_epoch_secs(&self, rel_path: &str) -> i64 {
        let repo = self.lock();
        walk_file_commits(&repo, rel_path, 1, |c, _| c.time().seconds())
            .ok()
            .and_then(|mut v| v.pop())
            .unwrap_or_else(|| {
                drop(repo);
                self.head_epoch_secs()
            })
    }

    /// Retrieve file content at a specific commit.
    pub fn blob_at(&self, rel_path: &str, oid: Oid) -> Result<Vec<u8>> {
        let repo = self.lock();
        let commit = repo.find_commit(oid).wrap_err("commit not found")?;
        let tree = commit.tree()?;
        let entry = tree
            .get_path(Path::new(rel_path))
            .wrap_err_with(|| format!("{rel_path} not in tree at {oid}"))?;
        let blob = repo.find_blob(entry.id())?;
        Ok(blob.content().to_vec())
    }

    /// Unique authors sorted by commit count for a given file.
    pub(super) fn contributors(&self, rel_path: &str) -> Result<Vec<Contributor>> {
        let repo = self.lock();
        let authors = walk_file_commits(&repo, rel_path, usize::MAX, |commit, _| {
            commit.author().name().unwrap_or("unknown").to_owned()
        })?;

        let mut counts: HashMap<String, usize> = HashMap::new();
        for author in authors {
            *counts.entry(author).or_default() += 1;
        }

        let mut result: Vec<Contributor> = counts
            .into_iter()
            .map(|(name, commits)| Contributor { name, commits })
            .collect();
        result.sort_by(|a, b| b.commits.cmp(&a.commits));
        Ok(result)
    }

    /// Collect git notes from commits that touched `rel_path`.
    pub(super) fn file_notes(&self, rel_path: &str, limit: usize) -> Result<Vec<NoteEntry>> {
        let repo = self.lock();
        let commits = walk_file_commits(&repo, rel_path, limit, |commit, oid| (oid, commit_info(commit, oid)))?;

        let mut entries = Vec::new();
        for (oid, info) in commits {
            let note = match repo.find_note(None, oid) {
                Ok(note) => note,
                Err(e) if e.code() == git2::ErrorCode::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            let Some(note_text) = note.message() else { continue };
            entries.push(NoteEntry {
                hash: info.hash,
                date: info.date,
                commit_message: info.message,
                note: note_text.to_owned(),
            });
        }
        Ok(entries)
    }

    /// Set or remove a git note on the most recent commit touching `rel_path`.
    ///
    /// Empty or whitespace-only `message` removes the existing note.
    pub(super) fn set_note(&self, rel_path: &str, message: &str) -> Result<()> {
        let repo = self.lock();
        let oid = first_file_commit(&repo, rel_path)?;
        let sig = repo.signature().wrap_err("git user.name/user.email not configured")?;

        if message.trim().is_empty() {
            // Remove note; ignore NotFound errors.
            if let Err(e) = repo.note_delete(oid, None, &sig, &sig)
                && e.code() != git2::ErrorCode::NotFound
            {
                return Err(e.into());
            }
        } else {
            repo.note(&sig, &sig, None, oid, message, true)?;
        }
        Ok(())
    }
}

fn blame_hunk(repo: &git2::Repository, hunk: &git2::BlameHunk<'_>) -> Result<BlameHunk> {
    let oid = hunk.orig_commit_id();
    let start = hunk.final_start_line();
    let end = start + hunk.lines_in_hunk() - 1;

    let lines = if start == end {
        format!("{start}")
    } else {
        format!("{start}-{end}")
    };

    let commit = if oid.is_zero() {
        CommitInfo {
            hash: format!("{oid:.7}"),
            author: "uncommitted".into(),
            date: "-".into(),
            message: "uncommitted changes".into(),
            epoch_secs: 0,
        }
    } else {
        let c = repo.find_commit(oid).wrap_err("blame commit lookup failed")?;
        commit_info(&c, oid)
    };

    Ok(BlameHunk {
        lines,
        start_line: start,
        end_line: end,
        commit,
    })
}

fn commit_entry(commit: &git2::Commit<'_>, oid: Oid) -> HistoryEntry {
    HistoryEntry {
        oid,
        commit: commit_info(commit, oid),
    }
}

/// Walk commits that touched `rel_path`, collecting results via `visit`.
///
/// Stops after `limit` successful visits or [`MAX_REVWALK`] total commits examined.
fn walk_file_commits<T>(
    repo: &git2::Repository,
    rel_path: &str,
    limit: usize,
    mut visit: impl FnMut(&git2::Commit<'_>, Oid) -> T,
) -> Result<Vec<T>> {
    let mut revwalk = repo.revwalk()?;
    revwalk
        .set_sorting(git2::Sort::TIME)
        .wrap_err("failed to configure revwalk")?;
    revwalk.push_head().wrap_err("HEAD not found")?;

    let mut results = Vec::new();
    for (walked, oid_result) in revwalk.enumerate() {
        if results.len() >= limit || walked >= MAX_REVWALK {
            break;
        }
        let oid = oid_result?;
        let commit = repo.find_commit(oid)?;
        if commit_touches_path(repo, &commit, rel_path)? {
            results.push(visit(&commit, oid));
        }
    }
    Ok(results)
}

/// Find the OID of the most recent commit that touched `rel_path`.
fn first_file_commit(repo: &git2::Repository, rel_path: &str) -> Result<Oid> {
    let mut results = walk_file_commits(repo, rel_path, 1, |_, oid| oid)?;
    results
        .pop()
        .ok_or_else(|| color_eyre::eyre::eyre!("no commits found touching {rel_path}"))
}

fn commit_touches_path(repo: &git2::Repository, commit: &git2::Commit<'_>, rel_path: &str) -> Result<bool> {
    let commit_tree = commit.tree()?;
    let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
    let mut opts = diff_opts(rel_path);
    let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&commit_tree), Some(&mut opts))?;
    Ok(diff.deltas().len() != 0)
}

/// Check whether a commit's diff for `rel_path` touches any lines in the given range.
fn commit_touches_range(repo: &git2::Repository, oid: Oid, rel_path: &str, range: &SymbolLineRange) -> Result<bool> {
    let commit = repo.find_commit(oid)?;
    let commit_tree = commit.tree()?;
    let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
    let mut opts = diff_opts(rel_path);
    let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&commit_tree), Some(&mut opts))?;

    let mut touches = false;
    let result = diff.foreach(
        &mut |_, _| true,
        None,
        Some(&mut |_delta, hunk| {
            // new_start/new_lines are the post-image line range (1-based).
            let hunk_start = hunk.new_start() as usize;
            let hunk_end = hunk_start + hunk.new_lines().max(1) as usize - 1;
            if hunk_start <= range.end && hunk_end >= range.start {
                touches = true;
            }
            !touches // stop iterating once we find a match
        }),
        None,
    );

    // git2 returns GIT_EUSER when a callback returns `false` to stop early.
    // That's our intentional early-exit, not a real error.
    match result {
        Ok(()) => Ok(touches),
        Err(e) if e.code() == git2::ErrorCode::User => Ok(touches),
        Err(e) => Err(e.into()),
    }
}
