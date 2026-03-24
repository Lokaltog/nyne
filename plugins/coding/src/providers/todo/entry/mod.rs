//! TODO entry — a single TODO marker in source code.

use std::path::PathBuf;

use nyne::text::slugify;
use nyne::types::path_conventions::COMPANION_SUFFIX;
use nyne::types::vfs_path::VfsPath;

use crate::providers::names::{DIR_TODO, SUBDIR_AT_LINE, SUBDIR_SYMBOLS};

/// A single TODO/FIXME/etc. found in a source file.
#[derive(Debug, Clone)]
pub(super) struct TodoEntry {
    /// Relative path of the source file (e.g., "src/main.rs").
    pub source_file: VfsPath,
    /// 1-based line number of the tag.
    pub line: usize,
    /// Which tag was matched (canonical case from config, e.g., "TODO", "FIXME").
    pub tag: String,
    /// Stripped comment text (no comment prefix, no tag prefix).
    pub text: String,
}

/// Methods for [`TodoEntry`].
impl TodoEntry {
    /// Filesystem-safe entry name: `src__main.rs:42--fix-frobnicator`
    pub fn fs_name(&self) -> String {
        let path_slug = self.source_file.as_str().replace('/', "__");
        let content_slug = slugify_content(&self.text);
        format!("{path_slug}:{}--{content_slug}", self.line)
    }

    /// Relative symlink target from `@/todo/<TAG>/<entry>` to the source
    /// file's at-line lookup (e.g., `src/main.rs@/symbols/at-line/42`).
    ///
    /// Both paths are mount-root-relative — the base includes the project
    /// root's `@` companion prefix since TODO entries aggregate across files.
    #[expect(clippy::expect_used, reason = "constructed from validated constants")]
    pub fn symlink_target(&self) -> PathBuf {
        let base = VfsPath::new(&format!("{COMPANION_SUFFIX}/{DIR_TODO}/{}", self.tag))
            .expect("todo base produced invalid VfsPath");
        let target = VfsPath::new(&format!(
            "{}{COMPANION_SUFFIX}/{SUBDIR_SYMBOLS}/{SUBDIR_AT_LINE}/{}",
            self.source_file, self.line
        ))
        .expect("todo target produced invalid VfsPath");
        target.relative_to(&base)
    }
}

/// Maximum kebab-case slug length for TODO entry filenames.
const TODO_SLUG_MAX_LEN: usize = 40;

/// Slugify the first line of comment text for use in filenames.
///
/// Delegates to [`nyne::text::slugify`] for slug conversion and truncation,
/// then falls back to `"unnamed"` for empty/whitespace-only input.
fn slugify_content(text: &str) -> String {
    let slug = slugify(text.lines().next().unwrap_or("").trim(), TODO_SLUG_MAX_LEN);
    if slug.is_empty() { "unnamed".to_owned() } else { slug }
}

/// Unit tests.
#[cfg(test)]
mod tests;
