//! Content splicing — replacing byte ranges in source files.

use std::io::{Error, ErrorKind};
use std::ops::Range;
use std::str::from_utf8;

use color_eyre::eyre::Result;
use crop::Rope;
use nyne::types::real_fs::RealFs;
use nyne::types::vfs_path::VfsPath;

/// Read a source file, splice new content at a byte range, validate, and write back.
///
/// The `validate` closure receives the full spliced source and should return
/// `Err` with a message if the result is invalid (rejected with `EINVAL`).
///
/// Returns the number of bytes in `new_content` on success.
pub fn splice_validate_write(
    real_fs: &dyn RealFs,
    source_file: &VfsPath,
    byte_range: Range<usize>,
    new_content: &str,
    validate: impl Fn(&str) -> Result<(), String>,
) -> Result<usize> {
    let current = real_fs.read(source_file)?;
    let current_str = from_utf8(&current)?;
    let mut rope = Rope::from(current_str);
    splice_rope_validate_write(real_fs, source_file, &mut rope, byte_range, new_content, validate)
}

/// Splice new content into source text at a byte range, returning the result.
///
/// This is the read-only core used by both [`splice_rope_validate_write`]
/// (which additionally validates and writes back) and diff previews (which
/// only need the modified text for diffing).
#[must_use]
#[cfg(test)]
pub fn splice_content(source: &str, byte_range: Range<usize>, new_content: &str) -> String {
    let mut rope = Rope::from(source);
    rope.replace(byte_range, new_content);
    rope.to_string()
}

/// Splice new content into a pre-built rope, validate, and write back.
///
/// Like [`splice_validate_write`] but accepts an already-loaded `Rope`,
/// avoiding a redundant file read + rope construction when the caller
/// needs the rope for preliminary work (e.g., line→byte offset conversion).
///
/// If the source already fails validation before the splice, post-splice
/// validation is skipped — edits to already-invalid files are always allowed.
///
/// Returns the number of bytes in `new_content` on success.
pub fn splice_rope_validate_write(
    real_fs: &dyn RealFs,
    source_file: &VfsPath,
    rope: &mut Rope,
    byte_range: Range<usize>,
    new_content: &str,
    validate: impl Fn(&str) -> Result<(), String>,
) -> Result<usize> {
    let source_len = rope.byte_len();
    if byte_range.start > source_len || byte_range.end > source_len {
        // Construct `io::Error` directly so that `fuse::extract_errno` can
        // find it in the eyre chain and map to `EINVAL`. This mirrors the
        // `io_err()` helper in `dispatch::mutation` — duplicated here because
        // `edit/` (Tier 1) cannot import from `dispatch/` (Tier 3).
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!(
                "byte range {}..{} out of bounds for source of length {source_len} \
                 (source={source_file})",
                byte_range.start, byte_range.end,
            ),
        )
        .into());
    }
    rope.replace(byte_range.clone(), new_content);
    let spliced = rope.to_string();

    // Validate the result. On failure, check whether the source was already
    // invalid before the splice — if so, allow the edit through (we can't
    // make it worse). This keeps the pre-splice `to_string()` off the
    // common (success) path.
    if let Err(e) = validate(&spliced) {
        // Re-read the original file (not yet overwritten) to check whether
        // it was already invalid before the splice. Only pay this cost on
        // the error path — the common (success) path does zero extra work.
        if validate(from_utf8(&real_fs.read(source_file)?).unwrap_or("")).is_ok() {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "{e} (source={source_file}, splice_range={}..{}, new_content_len={}, \
                     source_len={source_len}, result_len={})",
                    byte_range.start,
                    byte_range.end,
                    new_content.len(),
                    spliced.len(),
                ),
            )
            .into());
        }
    }
    real_fs.write(source_file, spliced.as_bytes())?;
    Ok(new_content.len())
}

/// Byte offset of the start of the line containing `offset`, using a pre-built rope.
///
/// Prefer this over [`line_start_of`] when the caller already has a `Rope`
/// or calls multiple line-offset functions on the same source.
#[must_use]
pub fn line_start_of_rope(rope: &Rope, offset: usize) -> usize { rope.byte_of_line(rope.line_of_byte(offset)) }

/// Byte offset of the start of the line containing `offset`.
///
/// Convenience wrapper that builds a [`crop::Rope`] internally. When calling
/// multiple line-offset functions on the same source, prefer
/// [`line_start_of_rope`] with a shared rope.
#[must_use]
pub fn line_start_of(source: &str, offset: usize) -> usize { line_start_of_rope(&Rope::from(source), offset) }

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

/// Byte offset of the end of the line containing `offset`.
///
/// Convenience wrapper that builds a [`crop::Rope`] internally. When calling
/// multiple line-offset functions on the same source, prefer
/// [`line_end_of_rope`] with a shared rope.
#[must_use]
pub fn line_end_of(source: &str, offset: usize) -> usize { line_end_of_rope(&Rope::from(source), offset) }

/// Determine the indentation prefix at a given byte offset, using a pre-built rope.
///
/// Finds the line start via the rope, then extracts leading whitespace from
/// `source`. Prefer this over [`indent_at`] when the caller already has a `Rope`.
#[must_use]
pub fn indent_at_rope<'a>(source: &'a str, rope: &Rope, offset: usize) -> &'a str {
    let line = &source[line_start_of_rope(rope, offset)..offset];
    &line[..line.find(|c: char| !c.is_whitespace()).unwrap_or(line.len())]
}

/// Determine the indentation prefix at a given byte offset in source text.
///
/// Convenience wrapper that builds a [`crop::Rope`] internally. When calling
/// multiple line-offset functions on the same source, prefer
/// [`indent_at_rope`] with a shared rope.
#[must_use]
pub fn indent_at(source: &str, offset: usize) -> &str { indent_at_rope(source, &Rope::from(source), offset) }

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
