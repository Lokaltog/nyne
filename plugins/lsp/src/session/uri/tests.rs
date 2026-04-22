use crop::Rope;
use lsp_types::Position;
use rstest::rstest;

use super::*;

/// `file_path_to_uri` and `text_document_id*` all produce URIs that
/// preserve the input path; `versioned_text_document_id` additionally
/// carries a version counter.
#[rstest]
#[case::file_uri("/tmp/src/main.rs")]
#[case::text_document("/tmp/lib.rs")]
fn path_becomes_uri(#[case] path: &str) {
    let p = Path::new(path);
    assert_eq!(file_path_to_uri(p).unwrap().path().as_str(), path);
    assert_eq!(text_document_id(p).unwrap().uri.path().as_str(), path);
}

#[rstest]
fn versioned_text_document_id_from_path() {
    let vtd = versioned_text_document_id(Path::new("/tmp/lib.rs"), 3).unwrap();
    assert_eq!(vtd.uri.path().as_str(), "/tmp/lib.rs");
    assert_eq!(vtd.version, 3);
}

/// `byte_offset_to_position` covers single-line, multi-line, and
/// UTF-16 surrogate-pair (emoji) inputs.
#[rstest]
// Single-line: "hello world"
#[case::single_line_mid("hello world", 6, Position { line: 0, character: 6 })]
#[case::single_line_start("hello world", 0, Position { line: 0, character: 0 })]
// Multi-line: "line one\nline two\nline three"
#[case::multiline_line1_start("line one\nline two\nline three", 9, Position { line: 1, character: 0 })]
#[case::multiline_line2_mid("line one\nline two\nline three", 23, Position { line: 2, character: 5 })]
// Emoji (U+1F600) is 4 bytes in UTF-8 and 2 code units in UTF-16.
#[case::utf16_before_emoji("a\u{1F600}b", 0, Position { line: 0, character: 0 })]
#[case::utf16_at_emoji("a\u{1F600}b", 1, Position { line: 0, character: 1 })]
#[case::utf16_after_emoji("a\u{1F600}b", 5, Position { line: 0, character: 3 })]
fn byte_offset_to_position_cases(#[case] text: &str, #[case] offset: usize, #[case] expected: Position) {
    assert_eq!(byte_offset_to_position(&Rope::from(text), offset), expected);
}

/// `position_to_byte_offset` covers basic conversion, out-of-range
/// (returns `None`), UTF-16 emoji, and past-EOL clamping.
#[rstest]
// Single-line basic.
#[case::single_line_mid("hello world", Position { line: 0, character: 6 }, Some(6))]
#[case::single_line_start("hello world", Position { line: 0, character: 0 }, Some(0))]
// Line past end-of-document.
#[case::out_of_range("one line", Position { line: 5, character: 0 }, None)]
// Emoji roundtrips.
#[case::utf16_after_emoji("a\u{1F600}b", Position { line: 0, character: 3 }, Some(5))]
#[case::utf16_at_emoji("a\u{1F600}b", Position { line: 0, character: 1 }, Some(1))]
// Column past line length clamps to EOL.
#[case::clamp_past_eol("short\nline", Position { line: 0, character: 100 }, Some(5))]
fn position_to_byte_offset_cases(#[case] text: &str, #[case] pos: Position, #[case] expected: Option<usize>) {
    assert_eq!(position_to_byte_offset(&Rope::from(text), pos), expected);
}

/// Verifies that byte offset and position conversions roundtrip correctly.
#[rstest]
fn position_byte_offset_roundtrip() {
    let text = "first line\nsecond line\nthird line";
    let rope = Rope::from(text);
    for target_offset in [0, 5, 11, 18, 22, 27] {
        let pos = byte_offset_to_position(&rope, target_offset);
        let recovered = position_to_byte_offset(&rope, pos);
        assert_eq!(
            recovered,
            Some(target_offset),
            "roundtrip failed for offset {target_offset} (line={}, col={})",
            pos.line,
            pos.character,
        );
    }
}
