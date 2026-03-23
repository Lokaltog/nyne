// URI and position conversion helpers for LSP protocol interop.
//
// SSOT: all file-path-to-URI and byte-offset-to-position conversions live here.
// Both `client.rs` and `edit.rs` import from this module.
//
// Position conversions delegate to `crop::Rope` with the `utf16-metric` feature
// for O(log n) line/column lookups instead of O(n) character scanning.

use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, eyre};
use crop::Rope;
use lsp_types::{Position, TextDocumentIdentifier, Uri, VersionedTextDocumentIdentifier};

/// Convert a filesystem path to an `lsp_types::Uri`.
pub fn file_path_to_uri(path: &Path) -> Result<Uri> {
    file_path_to_uri_string(path)?
        .parse()
        .map_err(|e| eyre!("failed to parse file URI into lsp_types::Uri: {e}"))
}

/// Convert a filesystem path to a `file://` URI string.
///
/// SSOT for path-to-URI conversion. Use this when `lsp_types::Uri` is not
/// needed (e.g., `FileRename` params which take raw strings).
pub fn file_path_to_uri_string(path: &Path) -> Result<String> {
    let url =
        url::Url::from_file_path(path).map_err(|()| eyre!("failed to convert path to file URI: {}", path.display()))?;
    Ok(url.to_string())
}

/// Extract a filesystem path from a `file://` URI.
///
/// SSOT for URI-to-path conversion — inverse of [`file_path_to_uri`].
pub fn uri_to_file_path(uri: &Uri) -> PathBuf {
    PathBuf::from(uri.as_str().strip_prefix("file://").unwrap_or(uri.as_str()))
}

/// Build a `TextDocumentIdentifier` from a filesystem path.
pub fn text_document_id(path: &Path) -> Result<TextDocumentIdentifier> {
    Ok(TextDocumentIdentifier {
        uri: file_path_to_uri(path)?,
    })
}

/// Build a `VersionedTextDocumentIdentifier` from a filesystem path and version.
pub fn versioned_text_document_id(path: &Path, version: i32) -> Result<VersionedTextDocumentIdentifier> {
    Ok(VersionedTextDocumentIdentifier {
        uri: file_path_to_uri(path)?,
        version,
    })
}

/// Convert an LSP `Position` (line, UTF-16 character offset) to a byte offset.
///
/// Uses `crop::Rope` for O(log n) line and UTF-16 code unit lookups.
/// Returns `None` if the line is out of range.
pub fn position_to_byte_offset(rope: &Rope, pos: Position) -> Option<usize> {
    let line = pos.line as usize;
    if line >= rope.line_len() {
        return None;
    }
    let line_start = rope.byte_of_line(line);
    let line_slice = rope.line(line);
    let utf16_col = pos.character as usize;
    // Clamp to line length so we don't panic on positions past EOL
    // (LSP servers sometimes report column == line length for EOL positions).
    let clamped = utf16_col.min(line_slice.utf16_len());
    Some(line_start + line_slice.byte_of_utf16_code_unit(clamped))
}

/// Convert a byte offset to an LSP `Position` (line, UTF-16 character offset).
///
/// Uses `crop::Rope` for O(log n) line and UTF-16 code unit lookups.
pub fn byte_offset_to_position(rope: &Rope, offset: usize) -> Position {
    let line = rope.line_of_byte(offset);
    let line_start = rope.byte_of_line(line);
    let prefix = rope.byte_slice(line_start..offset);
    let character = prefix.utf16_len();
    // LSP Position uses u32. Line/column counts in real files never exceed u32::MAX.
    Position {
        line: u32::try_from(line).unwrap_or(u32::MAX),
        character: u32::try_from(character).unwrap_or(u32::MAX),
    }
}

#[cfg(test)]
mod tests;
