//! Content splicing — replacing byte ranges in source files.
//!
//! The core splice operation reads source text, replaces a byte range with new
//! content via a `crop::Rope`, validates the result with a caller-supplied closure
//! (typically tree-sitter parsing), and writes back atomically. If the source was
//! already invalid before the splice, validation is skipped so edits to broken
//! files are always allowed. Helper functions provide line-start/end offset
//! calculation, indentation detection, and delete-range extension for whitespace
//! cleanup.

use std::io::ErrorKind;
use std::ops::Range;
use std::path::Path;

use color_eyre::eyre::Result;
use crop::Rope;
use nyne::err::io_err;
use nyne::router::Filesystem;

/// Splice new content into a pre-built rope, validate, and write back.
///
/// Like [`splice_validate_write`] but accepts an already-loaded `Rope`,
/// avoiding a redundant file read + rope construction when the caller
/// needs the rope for preliminary work (e.g., line→byte offset conversion).
///
/// Returns `Ok(new_content.len())` without writing when the splice would
/// produce byte-identical content — this avoids cascading cache invalidations
/// from no-op round trips (e.g. `cat body.rs > body.rs`).
///
/// If the source already fails validation before the splice, post-splice
/// validation is skipped — edits to already-invalid files are always allowed.
/// The pre-splice rope is retained as a cheap clone (`crop::Rope` clones
/// are O(1)) so the error path does not need to re-read the file.
pub fn splice_rope_validate_write(
    fs: &dyn Filesystem,
    source_file: &Path,
    rope: &mut Rope,
    byte_range: Range<usize>,
    new_content: &str,
    validate: impl Fn(&str) -> Result<(), String>,
) -> Result<usize> {
    let source_len = rope.byte_len();
    if byte_range.start > source_len || byte_range.end > source_len {
        return Err(io_err(
            ErrorKind::InvalidInput,
            format!(
                "byte range {}..{} out of bounds for source of length {source_len} \
                 (source={})",
                byte_range.start,
                byte_range.end,
                source_file.display(),
            ),
        ));
    }

    // No-op fast path: the splice would write byte-identical content. Skip
    // the write so downstream caches don't invalidate on unchanged files.
    // Trailing newline normalization is only needed when a byte is missing,
    // so check that too before claiming no-op.
    let trailing_newline_present = source_len == 0 || rope.byte(source_len - 1) == b'\n';
    if trailing_newline_present && rope.byte_slice(byte_range.clone()) == new_content {
        return Ok(new_content.len());
    }

    // Retain a cheap clone of the pre-splice rope so the error path can
    // check whether the source was already invalid without re-reading the
    // file (`crop::Rope` clones share structure, so this is O(1)).
    let pre_splice = rope.clone();

    rope.replace(byte_range.clone(), new_content);
    // Ensure the result ends with a newline. POSIX text file convention,
    // and tree-sitter grammars (notably markdown) treat a missing final
    // newline as a parse error.
    if rope.byte_len() > 0 && rope.byte(rope.byte_len() - 1) != b'\n' {
        rope.insert(rope.byte_len(), "\n");
    }
    let spliced = rope.to_string();

    // Validate the result. On failure, check whether the source was already
    // invalid before the splice — if so, allow the edit through (we can't
    // make it worse). This keeps the pre-splice `to_string()` off the
    // common (success) path.
    if let Err(e) = validate(&spliced)
        && validate(&pre_splice.to_string()).is_ok()
    {
        return Err(io_err(
            ErrorKind::InvalidInput,
            format!(
                "{e} (source={}, splice_range={}..{}, new_content_len={}, \
                 source_len={source_len}, result_len={})",
                source_file.display(),
                byte_range.start,
                byte_range.end,
                new_content.len(),
                spliced.len(),
            ),
        ));
    }
    fs.write_file(source_file, spliced.as_bytes())?;
    Ok(new_content.len())
}

/// Byte offset of the start of the line containing `offset`, using a pre-built rope.
///
/// Prefer this over [`line_start_of`] when the caller already has a `Rope`
/// or calls multiple line-offset functions on the same source.
#[must_use]
pub fn line_start_of_rope(rope: &Rope, offset: usize) -> usize { rope.byte_of_line(rope.line_of_byte(offset)) }

/// Byte offset of the end of the line containing `offset`, using a pre-built rope.
///
/// Returns the byte position just past the `\n` terminator (or `rope.byte_len()`
/// for the final unterminated line). Prefer this over [`line_end_of`] when the
/// caller already has a `Rope`.
#[must_use]
pub fn line_end_of_rope(rope: &Rope, offset: usize) -> usize {
    let line = rope.line_of_byte(offset);
    if line + 1 < rope.line_len() {
        rope.byte_of_line(line + 1)
    } else {
        rope.byte_len()
    }
}

/// Determine the indentation prefix at a given byte offset, using a pre-built rope.
///
/// Finds the line start via the rope, then extracts leading whitespace from
/// `source`. Prefer this over [`indent_at`] when the caller already has a `Rope`.
#[must_use]
pub fn indent_at_rope<'a>(source: &'a str, rope: &Rope, offset: usize) -> &'a str {
    let line = &source[line_start_of_rope(rope, offset)..offset];
    &line[..line.find(|c: char| !c.is_whitespace()).unwrap_or(line.len())]
}

/// Extend a byte range to consume trailing blank-line separators.
///
/// After removing a symbol we don't want orphan blank lines between the
/// preceding and following code. This scans forward from `span.end` and
/// absorbs any lines that consist entirely of whitespace.
#[must_use]
pub fn extend_delete_range(source: &str, span: Range<usize>) -> Range<usize> {
    let after = &source[span.end..];
    let mut extra = 0;
    for line in after.split_inclusive('\n') {
        if line.trim().is_empty() {
            extra += line.len();
        } else {
            break;
        }
    }
    span.start..span.end + extra
}

/// Unit tests for content splicing operations.
#[cfg(test)]
mod tests;
