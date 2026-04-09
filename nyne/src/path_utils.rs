//! Extension trait for `std::path::Path` with VFS-oriented helpers.
//!
//! Provides relative path computation and compound extension extraction
//! as methods on `Path`, usable across core and plugins.

use std::path::{Component, Path, PathBuf};

/// VFS-oriented path extensions.
pub trait PathExt {
    /// Compute the relative path from `base` to `self`.
    ///
    /// Both paths are treated as relative (no filesystem access). Produces
    /// `../` prefixes for each base component beyond the common prefix,
    /// then appends the remaining self components.
    ///
    /// ```
    /// # use std::path::{Path, PathBuf};
    /// # use nyne::path_utils::PathExt;
    /// assert_eq!(
    ///     Path::new("symbols/Foo@/body.rs").relative_to(Path::new("symbols/by-kind/fn")),
    ///     PathBuf::from("../../Foo@/body.rs"),
    /// );
    /// ```
    fn relative_to(&self, base: &Path) -> PathBuf;

    /// Extract the two rightmost extension segments from the filename.
    ///
    /// Returns `(inner, outer)` for compound extensions like
    /// `"template.md.j2"` → `("md", "j2")`. Returns `None` if the
    /// filename has fewer than two dots, or if either extension is empty.
    ///
    /// ```
    /// # use std::path::Path;
    /// # use nyne::path_utils::PathExt;
    /// assert_eq!(
    ///     Path::new("template.md.j2").compound_extension(),
    ///     Some(("md", "j2"))
    /// );
    /// assert_eq!(
    ///     Path::new("archive.tar.gz").compound_extension(),
    ///     Some(("tar", "gz"))
    /// );
    /// assert_eq!(Path::new("main.rs").compound_extension(), None);
    /// assert_eq!(Path::new("Makefile").compound_extension(), None);
    /// ```
    fn compound_extension(&self) -> Option<(&str, &str)>;

    /// Split into (`parent_dir`, `file_name_str`).
    ///
    /// Returns `("")` as the parent for root-level paths. Returns `None`
    /// if the path has no valid UTF-8 file name component.
    ///
    /// ```
    /// # use std::path::Path;
    /// # use nyne::path_utils::PathExt;
    /// assert_eq!(
    ///     Path::new("src/main.rs").split_dir_name(),
    ///     Some((Path::new("src"), "main.rs"))
    /// );
    /// assert_eq!(
    ///     Path::new("file.txt").split_dir_name(),
    ///     Some((Path::new(""), "file.txt"))
    /// );
    /// ```
    fn split_dir_name(&self) -> Option<(&Path, &str)>;

    /// Strip a suffix from the final component of the path.
    ///
    /// Returns `Some(cleaned)` if the suffix was present and the remaining
    /// name is non-empty, `None` otherwise.
    ///
    /// ```
    /// # use std::path::{Path, PathBuf};
    /// # use nyne::path_utils::PathExt;
    /// assert_eq!(
    ///     Path::new("src/Foo@").strip_name_suffix("@"),
    ///     Some(PathBuf::from("src/Foo")),
    /// );
    /// assert_eq!(Path::new("src/Foo").strip_name_suffix("@"), None);
    /// assert_eq!(Path::new("@").strip_name_suffix("@"), None);
    /// ```
    fn strip_name_suffix(&self, suffix: &str) -> Option<PathBuf>;

    /// Extract normal path components as owned strings.
    ///
    /// Filters out non-normal components (root `/`, `.`, `..`) and returns
    /// only the meaningful segments.
    ///
    /// ```
    /// # use std::path::Path;
    /// # use nyne::path_utils::PathExt;
    /// assert_eq!(Path::new("/foo/bar/baz").segments(), vec![
    ///     "foo", "bar", "baz"
    /// ],);
    /// assert_eq!(Path::new("a/b").segments(), vec!["a", "b"],);
    /// ```
    fn segments(&self) -> Vec<String>;

    /// Strip a `root` prefix from this path, returning the relative remainder.
    ///
    /// Returns `None` if this path is not under `root`, if the relative
    /// part is empty (path equals `root`), or if the relative part is not
    /// valid UTF-8.
    ///
    /// ```
    /// # use std::path::Path;
    /// # use nyne::path_utils::PathExt;
    /// assert_eq!(
    ///     Path::new("/home/user/project/src/main.rs").strip_root(Path::new("/home/user/project")),
    ///     Some(Path::new("src/main.rs")),
    /// );
    /// assert_eq!(
    ///     Path::new("/home/user/project").strip_root(Path::new("/home/user/project")),
    ///     None
    /// );
    /// assert_eq!(
    ///     Path::new("/other/path").strip_root(Path::new("/home/user/project")),
    ///     None
    /// );
    /// ```
    fn strip_root(&self, root: &Path) -> Option<&Path>;
}

impl PathExt for Path {
    fn relative_to(&self, base: &Path) -> PathBuf {
        let target_parts: Vec<_> = self.components().collect();
        let base_parts: Vec<_> = base.components().collect();
        let common = target_parts.iter().zip(&base_parts).take_while(|(a, b)| a == b).count();
        let mut result = PathBuf::new();
        for _ in 0..(base_parts.len() - common) {
            result.push("..");
        }
        for part in target_parts.get(common..).unwrap_or_default() {
            result.push(part);
        }
        result
    }

    fn compound_extension(&self) -> Option<(&str, &str)> {
        let name = self.file_name()?.to_str()?;
        let (rest, outer) = name.rsplit_once('.')?;
        let (_, inner) = rest.rsplit_once('.')?;
        if inner.is_empty() || outer.is_empty() {
            return None;
        }
        Some((inner, outer))
    }

    fn split_dir_name(&self) -> Option<(&Path, &str)> {
        let dir = self.parent().unwrap_or_else(|| Self::new(""));
        let name = self.file_name()?.to_str()?;
        Some((dir, name))
    }

    fn strip_name_suffix(&self, suffix: &str) -> Option<PathBuf> {
        let name = self.file_name()?.to_str()?;
        let stripped = name.strip_suffix(suffix)?;
        if stripped.is_empty() {
            return None;
        }
        Some(self.with_file_name(stripped))
    }

    fn segments(&self) -> Vec<String> {
        self.components()
            .filter_map(|c| match c {
                Component::Normal(s) => s.to_str().map(String::from),
                _ => None,
            })
            .collect()
    }

    fn strip_root(&self, root: &Path) -> Option<&Path> {
        let relative = self.strip_prefix(root).ok()?;
        if relative.as_os_str().is_empty() {
            return None;
        }
        relative.to_str()?;
        Some(relative)
    }
}
