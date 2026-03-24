//! Virtual filesystem relative paths with validation and normalization.

use std::fmt;
use std::path::PathBuf;

use color_eyre::eyre::{Result, bail};

/// Relative path within the virtual filesystem.
///
/// A thin newtype over `String` representing a normalized, relative path
/// with no provider semantics. The root is represented by an empty string
/// internally, displayed as `"/"`.
///
/// # Invariants
///
/// - Never absolute (no leading `/`)
/// - No `.` or `..` segments
/// - No double slashes
/// - No trailing slash (except root which is empty)
/// - No null bytes
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VfsPath(String);

/// Path construction, traversal, and query methods.
impl VfsPath {
    /// The root path.
    pub const fn root() -> Self { Self(String::new()) }

    /// Create a new `VfsPath` with validation.
    ///
    /// Accepts any relative path. Use [`root()`](Self::root) for the root path.
    ///
    /// # Errors
    ///
    /// Returns an error if the path is absolute, contains `.`/`..` segments,
    /// double slashes, null bytes, or a trailing slash.
    pub fn new(s: &str) -> Result<Self> {
        if s.is_empty() {
            return Ok(Self::root());
        }
        if s.starts_with('/') {
            bail!("VfsPath must be relative: {s:?}");
        }
        if s.ends_with('/') {
            bail!("VfsPath must not have trailing slash: {s:?}");
        }
        if s.contains("//") {
            bail!("VfsPath must not contain double slashes: {s:?}");
        }
        if s.contains('\0') {
            bail!("VfsPath must not contain null bytes");
        }
        if s.split('/').any(|seg| seg == ".." || seg == ".") {
            bail!("VfsPath must not contain '.' or '..' segments: {s:?}");
        }

        Ok(Self(s.to_owned()))
    }

    /// Whether this is the root path.
    pub const fn is_root(&self) -> bool { self.0.is_empty() }

    /// The number of path segments (root = 0).
    pub fn depth(&self) -> usize {
        if self.0.is_empty() {
            0
        } else {
            self.0.matches('/').count() + 1
        }
    }

    /// Return the parent path, or `None` if this is the root.
    pub fn parent(&self) -> Option<Self> {
        if self.0.is_empty() {
            return None;
        }
        Some(
            self.0
                .rsplit_once('/')
                .map_or_else(Self::root, |(parent, _)| Self(parent.to_owned())),
        )
    }

    /// Return the last component (file/directory name), or `None` if root.
    pub fn name(&self) -> Option<&str> {
        if self.0.is_empty() {
            return None;
        }
        Some(self.0.rsplit_once('/').map_or(self.0.as_str(), |(_, name)| name))
    }

    /// Join a single path segment onto this path.
    ///
    /// # Errors
    ///
    /// Returns an error if the segment is empty, contains `/`, `..`, or null bytes.
    pub fn join(&self, segment: &str) -> Result<Self> {
        if segment.is_empty() {
            bail!("cannot join empty segment");
        }
        if segment.contains('/') {
            bail!("join segment must not contain '/': {segment:?}");
        }
        if segment == ".." || segment == "." {
            bail!("join segment must not be '.' or '..'");
        }
        if segment.contains('\0') {
            bail!("join segment must not contain null bytes");
        }

        // Segment is validated — construct directly without re-validating.
        if self.0.is_empty() {
            Ok(Self(segment.to_owned()))
        } else {
            Ok(Self(format!("{}/{segment}", self.0)))
        }
    }

    /// Whether this path starts with the given prefix path.
    ///
    /// Compares on segment boundaries, not character boundaries:
    /// `"src/main"` starts with `"src"` but NOT `"sr"`.
    pub fn starts_with(&self, prefix: &Self) -> bool {
        if prefix.0.is_empty() {
            return true;
        }
        if self.0 == prefix.0 {
            return true;
        }
        self.0.starts_with(&prefix.0) && self.0.as_bytes().get(prefix.0.len()) == Some(&b'/')
    }

    /// Iterate over path segments. Empty iterator for root.
    pub fn components(&self) -> impl Iterator<Item = &str> { self.0.split('/').filter(|s| !s.is_empty()) }

    /// Collect path segments into a `Vec` for indexed access.
    pub fn segments(&self) -> Vec<&str> { self.components().collect() }

    /// Return the file extension (part after the last `.`), or `None` if the
    /// name has no dot or this is the root path.
    ///
    /// ```ignore
    /// VfsPath::new("src/main.rs")?.extension()    // Some("rs")
    /// VfsPath::new("archive.tar.gz")?.extension() // Some("gz")
    /// VfsPath::new("Makefile")?.extension()        // None
    /// VfsPath::root().extension()                  // None
    /// ```
    pub fn extension(&self) -> Option<&str> { self.name()?.rsplit_once('.').map(|(_, ext)| ext) }

    /// Return the two rightmost extension segments as `(inner, outer)`, or
    /// `None` if the name has fewer than two dots.
    ///
    /// ```ignore
    /// VfsPath::new("template.md.j2")?.compound_extension() // Some(("md", "j2"))
    /// VfsPath::new("archive.tar.gz")?.compound_extension() // Some(("tar", "gz"))
    /// VfsPath::new("main.rs")?.compound_extension()        // None
    /// VfsPath::new("Makefile")?.compound_extension()       // None
    /// ```
    pub fn compound_extension(&self) -> Option<(&str, &str)> {
        let name = self.name()?;
        let (rest, outer) = name.rsplit_once('.')?;
        let (_, inner) = rest.rsplit_once('.')?;
        if inner.is_empty() || outer.is_empty() {
            return None;
        }
        Some((inner, outer))
    }

    /// The inner string representation.
    pub fn as_str(&self) -> &str { &self.0 }

    /// Compute the relative path from `base` to `self`.
    ///
    /// Returns the path that, when resolved from `base` (treated as a
    /// directory), reaches `self`. The result may contain `..` segments.
    ///
    /// Both `self` and `base` must be relative to the same root.
    ///
    /// ```ignore
    /// let target = VfsPath::new("symbols/Foo@/body.rs")?;
    ///
    /// target.relative_to(&VfsPath::new("symbols/at-line")?)
    ///     // => PathBuf("../Foo@/body.rs")
    ///
    /// target.relative_to(&VfsPath::new("symbols/by-kind/function")?)
    ///     // => PathBuf("../../Foo@/body.rs")
    ///
    /// target.relative_to(&VfsPath::new("symbols")?)
    ///     // => PathBuf("Foo@/body.rs")
    /// ```
    pub fn relative_to(&self, base: &Self) -> PathBuf {
        let self_parts: Vec<&str> = self.components().collect();
        let base_parts: Vec<&str> = base.components().collect();

        // Find the longest common prefix.
        let common = self_parts.iter().zip(&base_parts).take_while(|(a, b)| a == b).count();

        // One `..` per remaining base component, then the remaining self components.
        let ups = base_parts.len() - common;
        let mut result = PathBuf::new();
        for _ in 0..ups {
            result.push("..");
        }
        for part in self_parts.iter().skip(common) {
            result.push(part);
        }
        result
    }
}

/// Displays root as `"/"` and all other paths as their inner string.
impl fmt::Display for VfsPath {
    /// Formats the value for display.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            f.write_str("/")
        } else {
            f.write_str(&self.0)
        }
    }
}

/// Debug-formats as `VfsPath("inner/path")`.
impl fmt::Debug for VfsPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "VfsPath({:?})", self.0) }
}

/// Borrows the inner string representation.
impl AsRef<str> for VfsPath {
    /// Returns the path as a string slice.
    fn as_ref(&self) -> &str { &self.0 }
}

/// Unit tests.
#[cfg(test)]
mod tests;
