//! Test helpers for nyne-source.

use crate::syntax::SyntaxRegistry;
use crate::syntax::fragment::DecomposedFile;

/// Build a `SyntaxRegistry` with all compiled-in languages.
///
/// Shorthand for [`SyntaxRegistry::build()`] so tests don't need to import
/// the registry type directly.
pub fn registry() -> SyntaxRegistry { SyntaxRegistry::build() }

/// Decompose `source` with the language registered for `ext` and return the
/// fragment tree. Panics if no language is registered for `ext`.
///
/// Shared helper used by per-language `basic()` fixtures.
pub fn decompose_fixture(ext: &str, source: &str) -> DecomposedFile {
    let (result, _tree) = registry().get(ext).unwrap().decompose(source, 5);
    result
}

use std::ops::Range;
use std::path::Path;
use std::str::from_utf8;

use color_eyre::eyre::Result;
use crop::Rope;
use nyne::router::Filesystem;

use crate::edit::splice::{line_start_of_rope, splice_rope_validate_write};

/// Read a source file, splice new content at a byte range, validate, and write back.
///
/// Test-only convenience: builds the rope from the file contents and delegates
/// to [`splice_rope_validate_write`].
pub fn splice_validate_write(
    fs: &dyn Filesystem,
    source_file: &Path,
    byte_range: Range<usize>,
    new_content: &str,
    validate: impl Fn(&str) -> Result<(), String>,
) -> Result<usize> {
    let content = fs.read_file(source_file)?;
    let mut rope = Rope::from(from_utf8(&content)?);
    splice_rope_validate_write(fs, source_file, &mut rope, byte_range, new_content, validate)
}

/// Splice new content into source text at a byte range, returning the result.
///
/// Test-only helper used by decomposition round-trip tests.
#[must_use]
pub fn splice_content(source: &str, byte_range: Range<usize>, new_content: &str) -> String {
    let mut rope = Rope::from(source);
    rope.replace(byte_range, new_content);
    rope.to_string()
}

/// Byte offset of the start of the line containing `offset`.
///
/// Test-only convenience wrapper that builds a [`Rope`] internally.
#[must_use]
pub fn line_start_of(source: &str, offset: usize) -> usize { line_start_of_rope(&Rope::from(source), offset) }
