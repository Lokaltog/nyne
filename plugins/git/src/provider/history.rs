//! History data types and git querying — blame hunks, commits, contributors, notes.

use std::collections::HashMap;
use std::path::Path;

use color_eyre::eyre::WrapErr;
use git2::Oid;
use nyne::node::Readable;
use nyne::prelude::*;
use nyne::types::SymbolLineRange;
use tracing::warn;

use crate::commit::{CommitInfo, commit_info, diff_opts};
use crate::repo::GitRepo;

/// Safety cap on revwalk iterations to prevent unbounded history walks.
const MAX_REVWALK: usize = 5000;

/// Maximum commits examined when computing contributors for a file.
const CONTRIBUTORS_LIMIT: usize = 500;

/// Blame hunk with line range and commit metadata.
///
/// Line numbers are 1-based inclusive, matching the template output.
/// The commit metadata is flattened into the serialized output via `#[serde(flatten)]`.
#[derive(serde::Serialize)]
pub struct BlameHunk {
    pub start_line: usize,
    pub end_line: usize,
    #[serde(flatten)]
    pub commit: CommitInfo,
}

/// A single history entry (commit that touched a file).
#[derive(serde::Serialize)]
pub struct HistoryEntry {
    #[serde(skip)]
    pub oid: Oid,
    pub hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
    /// Commit timestamp as seconds since epoch (not serialized into templates).
    #[serde(skip)]
    pub epoch_secs: i64,
}

/// An author with commit count.
#[derive(serde::Serialize)]
pub struct Contributor {
    pub name: String,
    pub commits: usize,
}

/// A git note attached to a commit.
#[derive(serde::Serialize)]
pub struct NoteEntry {
    pub hash: String,
    pub date: String,
    pub commit_message: String,
    pub note: String,
}

/// Returns the raw file content at a specific commit.
pub struct HistoryVersionContent {
    pub repo: Arc<GitRepo>,
    pub rel_path: Arc<str>,
    pub oid: Oid,
}

/// [`Readable`] implementation for [`HistoryVersionContent`].
impl Readable for HistoryVersionContent {
    /// Reads the file content at a specific historical commit.
    fn read(&self, _ctx: &RequestContext<'_>) -> Result<Vec<u8>> { self.repo.blob_at(&self.rel_path, self.oid) }
}

/// History-related queries on [`GitRepo`] — blame, file history, contributors, and notes.
///
/// Defined as an extension trait to make the module origin explicit: these methods
/// live in `provider::history` and are available wherever the trait is in scope.
pub trait HistoryQueries {
    /// Collect blame hunks for a file, including uncommitted changes.
    fn blame(&self, rel_path: &str) -> Result<Vec<BlameHunk>>;

    /// Commits that touched `rel_path`, newest first, capped at `limit`.
    fn file_history(&self, rel_path: &str, limit: usize) -> Result<Vec<HistoryEntry>>;

    /// Commits that touched `rel_path` within the given line range, newest first.
    fn file_history_in_range(&self, rel_path: &str, range: &SymbolLineRange, limit: usize)
    -> Result<Vec<HistoryEntry>>;

    /// Most recent commit that touched `rel_path`, as seconds since epoch.
    ///
    /// Falls back to HEAD time if the revwalk finds nothing.
    fn file_epoch_secs(&self, rel_path: &str) -> i64;

    /// Retrieve file content at a specific commit.
    fn blob_at(&self, rel_path: &str, oid: Oid) -> Result<Vec<u8>>;

    /// Unique authors sorted by commit count for a given file.
    fn contributors(&self, rel_path: &str) -> Result<Vec<Contributor>>;

    /// Collect git notes from commits that touched `rel_path`.
    fn file_notes(&self, rel_path: &str, limit: usize) -> Result<Vec<NoteEntry>>;

    /// Set or remove a git note on the most recent commit touching `rel_path`.
    ///
    /// Empty or whitespace-only `message` removes the existing note.
    fn set_note(&self, rel_path: &str, message: &str) -> Result<()>;
}

impl HistoryQueries for GitRepo {
    /// Collect blame hunks for a file, including uncommitted changes.
    ///
    /// Safe to call during FUSE callbacks — the repo is opened via
    /// the overlay merged path, so workdir reads resolve against the overlay
    /// (not through FUSE).
    fn blame(&self, rel_path: &str) -> Result<Vec<BlameHunk>> {
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
    fn file_history(&self, rel_path: &str, limit: usize) -> Result<Vec<HistoryEntry>> {
        let repo = self.lock();
        walk_file_commits(&repo, rel_path, limit, |commit, _oid| commit_entry(commit))
    }

    /// Commits that touched `rel_path` within the given line range, newest first.
    fn file_history_in_range(
        &self,
        rel_path: &str,
        range: &SymbolLineRange,
        limit: usize,
    ) -> Result<Vec<HistoryEntry>> {
        let repo = self.lock();
        let mut revwalk = repo.revwalk()?;
        revwalk
            .set_sorting(git2::Sort::TIME)
            .wrap_err("failed to configure revwalk")?;
        revwalk.push_head().wrap_err("HEAD not found")?;

        let mut opts = diff_opts(rel_path);
        let mut results = Vec::new();
        for (walked, oid_result) in revwalk.enumerate() {
            if results.len() >= limit || walked >= MAX_REVWALK {
                break;
            }
            let oid = oid_result?;
            let commit = repo.find_commit(oid)?;
            // Single diff: skip commits that don't touch the path, then check
            // whether the touched hunks overlap the requested line range.
            match diff_touches_range(&repo, &commit, &mut opts, range) {
                Ok(true) => results.push(commit_entry(&commit)),
                Ok(false) => {}
                Err(e) => {
                    warn!(oid = %oid, path = rel_path, error = %e, "commit range check failed");
                }
            }
        }
        Ok(results)
    }

    /// Most recent commit that touched `rel_path`, as seconds since epoch.
    ///
    /// Falls back to HEAD time if the revwalk finds nothing.
    fn file_epoch_secs(&self, rel_path: &str) -> i64 {
        let result = {
            let repo = self.lock();
            match walk_file_commits(&repo, rel_path, 1, |c, _| c.time().seconds()) {
                Ok(mut v) => v.pop(),
                Err(e) => {
                    warn!(path = rel_path, error = %e, "revwalk failed for file epoch");
                    None
                }
            }
        };
        result.unwrap_or_else(|| self.head_epoch_secs())
    }

    /// Retrieve file content at a specific commit.
    fn blob_at(&self, rel_path: &str, oid: Oid) -> Result<Vec<u8>> {
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
    fn contributors(&self, rel_path: &str) -> Result<Vec<Contributor>> {
        let authors = {
            let repo = self.lock();
            walk_file_commits(&repo, rel_path, CONTRIBUTORS_LIMIT, |commit, _| {
                commit.author().name().unwrap_or("unknown").to_owned()
            })?
        };

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
    fn file_notes(&self, rel_path: &str, limit: usize) -> Result<Vec<NoteEntry>> {
        let repo = self.lock();
        let mut entries = Vec::new();
        walk_file_commits(&repo, rel_path, limit, |commit, oid| {
            let note = match repo.find_note(None, oid) {
                Ok(note) => note,
                Err(e) if e.code() == git2::ErrorCode::NotFound => return,
                Err(e) => {
                    warn!(oid = %oid, "note lookup failed: {e}");
                    return;
                }
            };
            let Some(note_text) = note.message() else { return };
            let info = commit_info(commit);
            entries.push(NoteEntry {
                hash: info.hash,
                date: info.date,
                commit_message: info.message,
                note: note_text.to_owned(),
            });
        })?;
        Ok(entries)
    }

    /// Set or remove a git note on the most recent commit touching `rel_path`.
    ///
    /// Empty or whitespace-only `message` removes the existing note.
    fn set_note(&self, rel_path: &str, message: &str) -> Result<()> {
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

/// Convert a `git2::BlameHunk` into a [`BlameHunk`] with commit metadata.
fn blame_hunk(repo: &git2::Repository, hunk: &git2::BlameHunk<'_>) -> Result<BlameHunk> {
    let oid = hunk.orig_commit_id();
    let start = hunk.final_start_line();

    Ok(BlameHunk {
        start_line: start,
        end_line: start + hunk.lines_in_hunk() - 1,
        commit: if oid.is_zero() {
            CommitInfo::uncommitted(oid)
        } else {
            commit_info(&repo.find_commit(oid).wrap_err("blame commit lookup failed")?)
        },
    })
}

/// Convert a `git2::Commit` into a [`HistoryEntry`].
fn commit_entry(commit: &git2::Commit<'_>) -> HistoryEntry {
    let info = commit_info(commit);
    HistoryEntry {
        oid: commit.id(),
        hash: info.hash,
        author: info.author,
        date: info.date,
        message: info.message,
        epoch_secs: info.epoch_secs,
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

    let mut opts = diff_opts(rel_path);
    let mut results = Vec::new();
    for (walked, oid_result) in revwalk.enumerate() {
        if results.len() >= limit || walked >= MAX_REVWALK {
            break;
        }
        let oid = oid_result?;
        let commit = repo.find_commit(oid)?;
        if commit_touches_path(repo, &commit, &mut opts)? {
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

/// Compute the diff for a single commit's changes to `rel_path`.
fn commit_diff<'repo>(
    repo: &'repo git2::Repository,
    commit: &git2::Commit<'_>,
    opts: &mut git2::DiffOptions,
) -> Result<git2::Diff<'repo>> {
    let commit_tree = commit.tree()?;
    Ok(repo.diff_tree_to_tree(
        commit.parent(0).ok().and_then(|p| p.tree().ok()).as_ref(),
        Some(&commit_tree),
        Some(opts),
    )?)
}
/// Check whether a commit's diff touches the given path.
fn commit_touches_path(
    repo: &git2::Repository,
    commit: &git2::Commit<'_>,
    opts: &mut git2::DiffOptions,
) -> Result<bool> {
    Ok(commit_diff(repo, commit, opts)?.deltas().next().is_some())
}

/// Single-diff check: does this commit touch any lines in `range`?
///
/// Computes one diff and inspects its hunks — returns `false` early when the
/// diff has no deltas (commit doesn't touch the path at all), avoiding the
/// separate path-touch + range-touch double-diff that was here before.
fn diff_touches_range(
    repo: &git2::Repository,
    commit: &git2::Commit<'_>,
    opts: &mut git2::DiffOptions,
    range: &SymbolLineRange,
) -> Result<bool> {
    let diff = commit_diff(repo, commit, opts)?;

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
