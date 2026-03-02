use crop::Rope;
use lsp_types::Position;

use super::*;

#[test]
fn file_path_to_uri_absolute() {
    let uri = file_path_to_uri(Path::new("/tmp/src/main.rs")).unwrap();
    assert_eq!(uri.path().as_str(), "/tmp/src/main.rs");
}

#[test]
fn text_document_id_from_path() {
    let td = text_document_id(Path::new("/tmp/lib.rs")).unwrap();
    assert_eq!(td.uri.path().as_str(), "/tmp/lib.rs");
}

#[test]
fn versioned_text_document_id_from_path() {
    let vtd = versioned_text_document_id(Path::new("/tmp/lib.rs"), 3).unwrap();
    assert_eq!(vtd.uri.path().as_str(), "/tmp/lib.rs");
    assert_eq!(vtd.version, 3);
}

#[test]
fn byte_offset_to_position_basic() {
    let text = "hello world";
    let rope = Rope::from(text);
    assert_eq!(byte_offset_to_position(&rope, 6), Position { line: 0, character: 6 });
    assert_eq!(byte_offset_to_position(&rope, 0), Position { line: 0, character: 0 });
}

#[test]
fn byte_offset_to_position_multiline() {
    let text = "line one\nline two\nline three";
    let rope = Rope::from(text);
    // "line two" starts at byte 9, 'l' is line 1 col 0.
    assert_eq!(byte_offset_to_position(&rope, 9), Position { line: 1, character: 0 });
    // "line three" starts at byte 18, 't' at offset 23 is line 2 col 5.
    assert_eq!(byte_offset_to_position(&rope, 23), Position { line: 2, character: 5 });
}

#[test]
fn byte_offset_to_position_utf16_surrogate() {
    // Emoji (U+1F600) is 4 bytes in UTF-8 and 2 code units in UTF-16.
    let text = "a\u{1F600}b";
    let rope = Rope::from(text);
    // 'a' at offset 0 => (0, 0)
    assert_eq!(byte_offset_to_position(&rope, 0), Position { line: 0, character: 0 });
    // Emoji at offset 1 => (0, 1) — after 'a' which is 1 UTF-16 unit.
    assert_eq!(byte_offset_to_position(&rope, 1), Position { line: 0, character: 1 });
    // 'b' at offset 5 => (0, 3) — after 'a' (1) + emoji (2) = 3 UTF-16 units.
    assert_eq!(byte_offset_to_position(&rope, 5), Position { line: 0, character: 3 });
}

#[test]
fn position_to_byte_offset_basic() {
    let text = "hello world";
    let rope = Rope::from(text);
    assert_eq!(
        position_to_byte_offset(&rope, Position { line: 0, character: 6 }),
        Some(6)
    );
    assert_eq!(
        position_to_byte_offset(&rope, Position { line: 0, character: 0 }),
        Some(0)
    );
}

#[test]
fn position_to_byte_offset_out_of_range() {
    let text = "one line";
    let rope = Rope::from(text);
    assert_eq!(position_to_byte_offset(&rope, Position { line: 5, character: 0 }), None);
}

#[test]
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

#[test]
fn position_to_byte_offset_utf16_surrogate() {
    // Verify roundtrip with multi-byte UTF-16 characters.
    let text = "a\u{1F600}b";
    let rope = Rope::from(text);
    // 'b' is at byte offset 5, LSP position (0, 3)
    assert_eq!(
        position_to_byte_offset(&rope, Position { line: 0, character: 3 }),
        Some(5)
    );
    // Emoji starts at byte offset 1, LSP position (0, 1)
    assert_eq!(
        position_to_byte_offset(&rope, Position { line: 0, character: 1 }),
        Some(1)
    );
}

#[test]
fn position_to_byte_offset_clamps_past_eol() {
    let text = "short\nline";
    let rope = Rope::from(text);
    // Column 100 on a 5-char line should clamp to end of line (byte 5).
    assert_eq!(
        position_to_byte_offset(&rope, Position {
            line: 0,
            character: 100
        }),
        Some(5)
    );
}
