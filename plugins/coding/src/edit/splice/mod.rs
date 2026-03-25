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
    // Check whether the source already has errors before splicing.
    // If it does, skip post-splice validation — we can't make it worse.
    let pre_splice = rope.to_string();
    let already_invalid = validate(&pre_splice).is_err();

    rope.replace(byte_range.clone(), new_content);
    let spliced = rope.to_string();
    if !already_invalid {
        validate(&spliced).map_err(|e| {
            // io::Error for FUSE errno extraction — see comment above.
            Error::new(
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
        })?;
    }
    real_fs.write(source_file, spliced.as_bytes())?;
    Ok(new_content.len())
}

/// Byte offset of the start of the line containing `offset`.
///
/// Scans backward from `offset` to the preceding `\n` (or start of string).
/// Used to snap byte-range starts to line boundaries so that sliced content
/// preserves consistent indentation on every line.
#[must_use]
pub fn line_start_of(source: &str, offset: usize) -> usize { source[..offset].rfind('\n').map_or(0, |pos| pos + 1) }

/// Byte offset of the end of the line containing `offset`.
///
/// Scans forward from `offset` to the next `\n` (inclusive) or end of string.
/// Used to snap byte-range ends to line boundaries for `SpliceMode::Byte`
/// masking, where the full lines are read but out-of-span bytes are replaced
/// with spaces.
#[must_use]
pub fn line_end_of(source: &str, offset: usize) -> usize {
    source[offset..].find('\n').map_or(source.len(), |pos| offset + pos + 1)
}

/// Determine the indentation prefix at a given byte offset in source text.
///
/// Scans backward from `offset` to the start of the line, collecting
/// whitespace characters. Used to preserve indentation when wrapping
/// doc comments or inserting content.
#[must_use]
pub fn indent_at(source: &str, offset: usize) -> &str {
    let before = &source[..offset];
    let line_start = before.rfind('\n').map_or(0, |pos| pos + 1);
    let line = &source[line_start..offset];
    let non_ws = line.find(|c: char| !c.is_whitespace()).unwrap_or(line.len());
    &source[line_start..line_start + non_ws]
}

/// Extend a byte range to consume trailing blank-line separators.
///
/// After removing a symbol we don't want orphan blank lines between the
/// preceding and following code. This scans forward from `span.end` and
/// absorbs any lines that consist entirely of whitespace.
#[must_use]
pub fn extend_delete_range(source: &str, span: &Range<usize>) -> Range<usize> {
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
