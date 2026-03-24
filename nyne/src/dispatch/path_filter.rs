//! Fast path filter for skipping provider resolution on ignored paths.
//!
//! The [`PathFilter`] is built once at mount time by walking the project tree
//! to discover `.gitignore` files and building in-memory matchers from them
//! (via the `ignore` crate). During FUSE operations, the router consults the
//! filter to short-circuit provider resolution for paths that are gitignored
//! or inside the git directory — these paths are always passthrough.
//!
//! # Staleness
//!
//! The filter is a snapshot of ignore rules at mount time. If `.gitignore`
//! files change while mounted, the filter becomes stale. This is acceptable
//! because:
//! - False negatives (missing a newly-ignored path) only cause a performance
//!   hit — providers still return no virtual content, falling through to
//!   passthrough.
//! - False positives (skipping a newly-unignored path) are self-correcting
//!   on remount.
//!
//! TODO: Rebuild matchers on `.gitignore` change events from the watcher
//! for live accuracy without remount.

use std::cmp;
use std::path::{Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use tracing::{debug, trace, warn};

use crate::types::vfs_path::VfsPath;

/// Pre-computed filter for paths that should bypass provider resolution.
///
/// Holds a chain of `Gitignore` matchers (one per `.gitignore` file found
/// in the project tree), plus the git directory name for direct matching.
/// All matching is pure in-memory computation — no locks, no I/O.
pub struct PathFilter {
    /// VFS-relative first component of the git directory (usually `.git`).
    /// `None` if the project is not a git repo or the git dir is outside
    /// the project tree (e.g., `GIT_DIR` pointing elsewhere).
    git_dir_component: Option<String>,

    /// Gitignore matchers ordered from deepest (most specific) to shallowest
    /// (root). Each matcher is rooted at the directory containing its
    /// `.gitignore` file. Includes `.git/info/exclude` and global gitignore.
    matchers: Vec<Gitignore>,
}

/// Construction and query methods for the gitignore-based path filter.
impl PathFilter {
    /// Build a path filter by walking the project tree.
    ///
    /// - `overlay_root`: absolute path to the overlay merged directory
    ///   (where the daemon performs I/O).
    /// - `git_dir_component`: the VFS-relative name of the git directory
    ///   (e.g., `".git"`). Provided by the git plugin via [`GitDirName`](crate::types::GitDirName).
    pub(crate) fn build(overlay_root: &Path, git_dir_component: Option<String>) -> Self {
        let matchers = build_gitignore_matchers(overlay_root, git_dir_component.as_deref());
        debug!(
            matcher_count = matchers.len(),
            git_dir = ?git_dir_component,
            "path filter built"
        );
        Self {
            git_dir_component,
            matchers,
        }
    }

    /// Returns `true` if the path should skip provider resolution
    /// (i.e., go straight to passthrough).
    pub(crate) fn is_skippable(&self, path: &VfsPath) -> bool {
        if path.is_root() {
            return false;
        }

        // Fast path: git directory itself.
        if let Some(git_dir) = &self.git_dir_component
            && path.components().next().is_some_and(|c| c == git_dir)
        {
            return true;
        }

        // Check gitignore matchers from most specific to least specific.
        // `matched_path_or_any_parents` walks up the path components, so
        // if `target/` is ignored, `target/debug/foo.o` matches immediately
        // without checking each intermediate directory.
        let abs_path = Path::new(path.as_str());
        let is_dir = false; // Conservative: treat as file. Gitignore `dir/`
        // patterns won't match, but `dir` patterns will.
        // The router handles directory detection separately.
        for matcher in &self.matchers {
            match matcher.matched_path_or_any_parents(abs_path, is_dir) {
                ignore::Match::Ignore(_) => return true,
                ignore::Match::Whitelist(_) => return false,
                ignore::Match::None => {}
            }
        }

        false
    }
}

/// Build a `WalkBuilder` that walks non-ignored directories, skipping
/// the git directory.
///
/// Shared by both [`PathFilter::build`] (to discover `.gitignore` files)
/// and the filesystem watcher (to install inotify watches).
///
/// - `walk_root`: the directory to start walking from.
/// - `filter_root`: the project root used to compute relative paths for
///   the `.git` filter (may differ from `walk_root` for dynamic watches).
/// - `git_dir`: the git directory name to filter out (e.g., `.git`).
pub fn ignore_walk_builder(walk_root: &Path, filter_root: &Path, git_dir: &str) -> ignore::WalkBuilder {
    let filter_root = filter_root.to_path_buf();
    let git_dir = git_dir.to_owned();
    let mut builder = ignore::WalkBuilder::new(walk_root);
    builder.hidden(false).filter_entry(move |entry| {
        let Ok(rel) = entry.path().strip_prefix(&filter_root) else {
            return true;
        };
        // Skip the git directory — WalkBuilder does NOT skip it when
        // hidden(false) is set.
        rel.as_os_str().is_empty() || !rel.starts_with(git_dir.as_str())
    });
    builder
}

/// Walk the project tree to discover `.gitignore` files and build matchers.
///
/// Returns matchers ordered from deepest (most specific) to shallowest.
/// Includes `.git/info/exclude` and the global gitignore.
fn build_gitignore_matchers(overlay_root: &Path, git_dir: Option<&str>) -> Vec<Gitignore> {
    let git_dir_name = git_dir.unwrap_or(".git");

    // Walk the tree to find .gitignore files in non-ignored directories.
    let mut gitignore_paths: Vec<PathBuf> = ignore_walk_builder(overlay_root, overlay_root, git_dir_name)
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_some_and(|ft| !ft.is_dir()))
        .filter(|e| e.path().file_name().is_some_and(|n| n == ".gitignore"))
        .map(ignore::DirEntry::into_path)
        .collect();

    // Sort by depth (deepest first) for most-specific-first matching.
    gitignore_paths.sort_by_key(|p| cmp::Reverse(p.components().count()));

    let mut matchers: Vec<Gitignore> = gitignore_paths
        .iter()
        .filter_map(|path| {
            let dir = path.parent().unwrap_or(overlay_root);
            let gi = try_build_gitignore(dir, path)?;
            trace!(path = %path.display(), "loaded .gitignore");
            Some(gi)
        })
        .collect();

    // Add .git/info/exclude if it exists.
    let exclude_path = overlay_root.join(git_dir_name).join("info/exclude");
    if exclude_path.is_file()
        && let Some(gi) = try_build_gitignore(overlay_root, &exclude_path)
    {
        trace!("loaded .git/info/exclude");
        matchers.push(gi);
    }

    // Add global gitignore (core.excludesFile or XDG default).
    let (global, err) = GitignoreBuilder::new(overlay_root).build_global();
    if let Some(err) = err {
        warn!(error = %err, "failed to parse global gitignore");
    }
    if !global.is_empty() {
        trace!("loaded global gitignore");
        matchers.push(global);
    }

    matchers
}

/// Parse a single gitignore file into a matcher, logging on failure.
///
/// Returns `None` if the file cannot be parsed, fails to build, or
/// produces an empty matcher.
fn try_build_gitignore(base: &Path, path: &Path) -> Option<Gitignore> {
    let mut builder = GitignoreBuilder::new(base);
    if let Some(err) = builder.add(path) {
        warn!(path = %path.display(), error = %err, "failed to parse gitignore");
        return None;
    }
    match builder.build() {
        Ok(gi) if !gi.is_empty() => Some(gi),
        Ok(_) => None,
        Err(err) => {
            warn!(path = %path.display(), error = %err, "failed to build gitignore matcher");
            None
        }
    }
}
