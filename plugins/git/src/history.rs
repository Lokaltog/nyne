//! History data types and git querying — blame hunks, commits, contributors, notes.
//!
//! Defines the [`HistoryQueries`] extension trait on [`Repo`] providing
//! blame, file history, contributor ranking, and git notes. All queries
//! operate on pre-mount real paths. History walks are capped at
//! [`MAX_REVWALK`] to prevent unbounded traversal on repositories with
//! deep commit histories.
//!
//! [`HistoryVersionContent`] provides readable blob content at arbitrary
//! commits, enabling the `file.rs@/history/` VFS directory.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::path::Path;
use std::str::from_utf8;
use std::sync::Arc;

use color_eyre::eyre::{Result, WrapErr};
use git2::Oid;
use nyne::router::{ReadContext, Readable};
use nyne::{SliceSpec, SymbolLineRange};
use nyne_source::SyntaxRegistry;
use tracing::warn;

use crate::commit::{CommitInfo, commit_info, diff_opts};
use crate::repo::Repo;

/// Safety cap on revwalk iterations to prevent unbounded history walks.
const MAX_REVWALK: usize = 5000;

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
/// Slice blame hunks by [`SliceSpec`] (source line range).
///
/// Converts the spec into a 1-based inclusive line range using the total
/// line count from the hunks, then filters/clips to that range.
pub fn slice_blame_hunks(hunks: Vec<BlameHunk>, spec: &SliceSpec) -> Vec<BlameHunk> {
    let range = spec.index_range(hunks.last().map_or(0, |h| h.end_line));
    // index_range returns 0-based half-open; blame lines are 1-based inclusive.
    clamp_blame(hunks, range.start + 1, range.end)
}

/// Clamp blame hunks to a 1-based inclusive line range `[start, end]`,
/// dropping hunks outside the range entirely.
fn clamp_blame(hunks: Vec<BlameHunk>, start: usize, end: usize) -> Vec<BlameHunk> {
    hunks
        .into_iter()
        .filter(|h| h.start_line <= end && h.end_line >= start)
        .map(|mut h| {
            h.start_line = h.start_line.max(start);
            h.end_line = h.end_line.min(end);
            h
        })
        .collect()
}
/// Filter blame hunks to those overlapping a symbol line range, clamping boundaries.
///
/// Similar to [`slice_blame_hunks`] but takes a [`SymbolLineRange`] directly
/// rather than a [`SliceSpec`].
pub fn filter_blame_to_range(hunks: Vec<BlameHunk>, range: &SymbolLineRange) -> Vec<BlameHunk> {
    clamp_blame(hunks, range.start, range.end)
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
pub struct Contributor {
    pub name: String,
    pub commits: usize,
}

/// A git note attached to a commit.
#[derive(serde::Serialize)]
pub struct NoteEntry {
    #[serde(flatten)]
    pub commit: CommitInfo,
    pub note: String,
}

/// Context for extracting a symbol body from a historical file revision.
///
/// Shared (via `Arc`) across all [`HistoryVersionContent`] nodes for a symbol's
/// history directory, avoiding redundant registry and path lookups per version.
pub struct SymbolExtractCtx {
    pub syntax: Arc<SyntaxRegistry>,
    pub ext: String,
    pub fragment_path: Arc<[String]>,
    pub max_depth: usize,
}
/// Returns file content at a specific commit, optionally extracting a symbol
/// body using tree-sitter decomposition.
pub struct HistoryVersionContent {
    pub repo: Arc<Repo>,
    pub rel_path: Arc<str>,
    pub oid: Oid,
    /// When present, extracts a specific symbol body from the historical blob
    /// using tree-sitter decomposition instead of returning the full file.
    pub symbol_ctx: Option<Arc<SymbolExtractCtx>>,
}

/// [`Readable`] implementation for [`HistoryVersionContent`].
///
/// When [`symbol_ctx`](HistoryVersionContent::symbol_ctx) is present, extracts
/// the symbol body from the historical blob via tree-sitter decomposition.
/// Falls back to the full file content if the blob is not valid UTF-8, no
/// decomposer exists for the extension, or the symbol path doesn't match.
impl Readable for HistoryVersionContent {
    fn read(&self, _ctx: &ReadContext<'_>) -> Result<Vec<u8>> {
        let blob = self.repo.blob_at(&self.rel_path, self.oid)?;
        let Some(sym) = &self.symbol_ctx else {
            return Ok(blob);
        };
        let Ok(source) = from_utf8(&blob) else {
            return Ok(blob);
        };
        match sym
            .syntax
            .extract_symbol(source, &sym.ext, &sym.fragment_path, sym.max_depth)
        {
            Some(body) => Ok(body.into_bytes()),
            None => Ok(blob),
        }
    }
}

/// History-related queries on [`Repo`] — blame, file history, contributors, and notes.
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

    /// Retrieve file content at a specific commit.
    fn blob_at(&self, rel_path: &str, oid: Oid) -> Result<Vec<u8>>;

    /// Unique authors sorted by commit count for a given file.
    ///
    /// `limit` caps how many commits are examined.
    fn contributors(&self, rel_path: &str, limit: usize) -> Result<Vec<Contributor>>;

    /// Collect git notes from commits that touched `rel_path`.
    fn file_notes(&self, rel_path: &str, limit: usize) -> Result<Vec<NoteEntry>>;

    /// Set or remove a git note on the most recent commit touching `rel_path`.
    ///
    /// Empty or whitespace-only `message` removes the existing note.
    fn set_note(&self, rel_path: &str, message: &str) -> Result<()>;
}

/// [`HistoryQueries`] implementation for [`Repo`].
///
/// All methods operate on the real filesystem (pre-mount paths) to avoid
/// FUSE recursion. History walks are capped at [`MAX_REVWALK`] iterations
/// to prevent unbounded traversal on large repositories.
impl HistoryQueries for Repo {
    /// Collect blame hunks for a file, including uncommitted changes.
    ///
    /// Safe to call during FUSE callbacks — the repo is opened via
    /// the source root, so workdir reads resolve against the backing
    /// filesystem (not through FUSE).
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
        walk_file_commits(&repo, rel_path, limit, |commit, oid| HistoryEntry {
            oid,
            commit: commit_info(commit),
        })
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
                Ok(true) => results.push(HistoryEntry { oid, commit: commit_info(&commit) }),
                Ok(false) => {}
                Err(e) => {
                    warn!(oid = %oid, path = rel_path, error = %e, "commit range check failed");
                }
            }
        }
        Ok(results)
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

    fn contributors(&self, rel_path: &str, limit: usize) -> Result<Vec<Contributor>> {
        let authors = {
            let repo = self.lock();
            walk_file_commits(&repo, rel_path, limit, |commit, _| {
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
        result.sort_by_key(|x| Reverse(x.commits));
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
            entries.push(NoteEntry {
                commit: commit_info(commit),
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
        let oid = walk_file_commits(&repo, rel_path, 1, |_, oid| oid)?
            .pop()
            .ok_or_else(|| color_eyre::eyre::eyre!("no commits found touching {rel_path}"))?;
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
        if commit_diff(repo, &commit, &mut opts)?.deltas().next().is_some() {
            results.push(visit(&commit, oid));
        }
    }
    Ok(results)
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
