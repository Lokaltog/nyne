//! Fast path filter for skipping virtual-content decoration on ignored paths.
//!
//! The [`PathFilter`] is built once at mount time by walking the project
//! tree to discover `.gitignore` files and building in-memory matchers
//! via the `ignore` crate. Middlewares consult it to short-circuit their
//! own virtual-content work for paths that should pass through directly
//! to the underlying filesystem.
//!
//! # Staleness
//!
//! The filter is a snapshot of ignore rules at mount time. If `.gitignore`
//! files change while mounted, the filter becomes stale. Rebuilding
//! requires a remount. False negatives (missing a newly-ignored path)
//! only cause extra work — virtual content is still correct. False
//! positives (skipping a newly-unignored path) are self-correcting on
//! remount.

use std::cmp;
use std::path::{Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use tracing::{debug, trace, warn};

/// Pre-computed filter for paths that should bypass virtual-content
/// decoration.
///
/// Holds a chain of `Gitignore` matchers — one per `.gitignore` file
/// found in the project tree, plus `.git/info/exclude`, the global
/// gitignore, and a synthetic matcher compiled from
/// [`MountConfig::excluded_patterns`](crate::config::MountConfig::excluded_patterns).
/// All matching is pure in-memory computation — no locks, no I/O.
pub struct PathFilter {
    source_root: PathBuf,
    /// Matchers ordered from deepest (most specific) to shallowest
    /// (root), then `.git/info/exclude`, then git global, then user
    /// `excluded_patterns`.
    matchers: Vec<Gitignore>,
}

impl PathFilter {
    /// Build a path filter rooted at `source_root`.
    ///
    /// Sources (in precedence order, deepest first):
    /// - Every `.gitignore` file under `source_root`.
    /// - `<source_root>/.git/info/exclude` if present.
    /// - Git global ignore (via `core.excludesFile` / XDG default).
    /// - `excluded_patterns` compiled as a dedicated matcher rooted at
    ///   `source_root`, so values from
    ///   [`MountConfig::excluded_patterns`](crate::config::MountConfig::excluded_patterns)
    ///   are honoured alongside gitignore rules.
    pub fn build(source_root: &Path, excluded_patterns: &[String]) -> Self {
        let mut matchers = build_gitignore_matchers(source_root);
        if let Some(gi) = build_excluded_patterns_matcher(source_root, excluded_patterns) {
            matchers.push(gi);
        }
        debug!(
            matcher_count = matchers.len(),
            source_root = %source_root.display(),
            "path filter built"
        );
        Self {
            source_root: source_root.to_path_buf(),
            matchers,
        }
    }

    /// Build an empty filter that excludes nothing.
    ///
    /// Used as the default when no mount-time filter is available
    /// (tests, minimal chains).
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            source_root: PathBuf::new(),
            matchers: Vec::new(),
        }
    }

    /// Returns `true` if `path` should bypass virtual-content decoration.
    ///
    /// `path` may be given as either a project-relative path (as
    /// delivered by FUSE requests) or an absolute path under the source
    /// root. Relative paths are resolved against the configured
    /// `source_root` before matching so gitignore rules see the same
    /// rooted layout they were built with.
    pub fn is_excluded(&self, path: &Path) -> bool {
        if self.matchers.is_empty() {
            return false;
        }
        if path.as_os_str().is_empty() || path == Path::new("/") {
            return false;
        }
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.source_root.join(path)
        };

        // Pass `is_dir: true` so directory-only patterns
        // (`node_modules/`, `target/`, `build/`) match the leaf
        // component when the query targets the directory itself
        // (e.g. a `readdir` on `node_modules`), not only descendants
        // discovered via ancestor walking. Gitignore has no file-only
        // pattern syntax — bare patterns match both files and
        // directories — so the only over-match risk is a dir-only glob
        // matching a same-named file, which is both exceedingly rare
        // and harmless (at worst, a file is treated as ignored).
        //
        // Skip matchers whose root does not cover `abs`: a nested
        // `.gitignore` at `a/.gitignore` only governs paths inside
        // `a/`, and `matched_path_or_any_parents` panics when called
        // with a path outside its matcher root.
        for matcher in &self.matchers {
            if !abs.starts_with(matcher.path()) {
                continue;
            }
            match matcher.matched_path_or_any_parents(&abs, true) {
                ignore::Match::Ignore(_) => {
                    trace!(path = %abs.display(), "path filter: excluded");
                    return true;
                }
                ignore::Match::Whitelist(_) => return false,
                ignore::Match::None => {}
            }
        }
        false
    }
}

impl Default for PathFilter {
    fn default() -> Self { Self::empty() }
}

/// Walk the project tree to discover `.gitignore` files and build
/// matchers, including `.git/info/exclude` and the git global ignore.
///
/// Matchers are returned deepest-first so the most specific rules are
/// checked before broader ones.
fn build_gitignore_matchers(source_root: &Path) -> Vec<Gitignore> {
    let mut gitignore_paths: Vec<PathBuf> = ignore::WalkBuilder::new(source_root)
        .hidden(false)
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_some_and(|ft| !ft.is_dir()))
        .filter(|e| e.path().file_name().is_some_and(|n| n == ".gitignore"))
        .map(ignore::DirEntry::into_path)
        .collect();

    gitignore_paths.sort_by_key(|p| cmp::Reverse(p.components().count()));

    let mut matchers: Vec<Gitignore> = gitignore_paths
        .iter()
        .filter_map(|path| {
            let gi = try_build_gitignore(path.parent().unwrap_or(source_root), path)?;
            trace!(path = %path.display(), "loaded .gitignore");
            Some(gi)
        })
        .collect();

    let exclude_path = source_root.join(".git").join("info").join("exclude");
    if exclude_path.is_file()
        && let Some(gi) = try_build_gitignore(source_root, &exclude_path)
    {
        trace!("loaded .git/info/exclude");
        matchers.push(gi);
    }

    let (global, err) = GitignoreBuilder::new(source_root).build_global();
    if let Some(err) = err {
        warn!(error = %err, "failed to parse global gitignore");
    }
    if !global.is_empty() {
        trace!("loaded git global ignore");
        matchers.push(global);
    }

    matchers
}

/// Compile `excluded_patterns` into a synthetic gitignore matcher.
///
/// Returns `None` if the slice is empty or every pattern fails to parse.
fn build_excluded_patterns_matcher(source_root: &Path, patterns: &[String]) -> Option<Gitignore> {
    if patterns.is_empty() {
        return None;
    }
    let mut builder = GitignoreBuilder::new(source_root);
    let mut valid = 0_usize;
    for pat in patterns {
        match builder.add_line(None, pat) {
            Ok(_) => valid += 1,
            Err(err) => warn!(pattern = %pat, error = %err, "invalid excluded_pattern"),
        }
    }
    if valid == 0 {
        return None;
    }
    match builder.build() {
        Ok(gi) if !gi.is_empty() => {
            trace!(count = valid, "loaded excluded_patterns");
            Some(gi)
        }
        Ok(_) => None,
        Err(err) => {
            warn!(error = %err, "failed to build excluded_patterns matcher");
            None
        }
    }
}

/// Parse a single gitignore file into a matcher, logging on failure.
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

#[cfg(test)]
mod tests;
