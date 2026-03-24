/// Simulate `AppendTrailingNewline::process_read` — just appends `\n`.
fn read(mut data: Vec<u8>) -> Vec<u8> {
    data.push(b'\n');
    data
}

/// Simulate `StripTrailingNewline::process_write` — strips one trailing `\n`.
fn write(mut data: Vec<u8>) -> Vec<u8> {
    if data.ends_with(b"\n") {
        data.pop();
    }
    data
}

/// Tests round-trip: content without trailing newline is preserved.
#[test]
fn round_trip_no_trailing_newline() {
    let original = b"fn foo() {}".to_vec();
    let after_read = read(original.clone());
    assert_eq!(&after_read, b"fn foo() {}\n");
    assert_eq!(write(after_read), original);
}

/// Tests round-trip: content with trailing newline is preserved.
#[test]
fn round_trip_with_trailing_newline() {
    let original = b"## Section\n\nContent.\n".to_vec();
    let after_read = read(original.clone());
    assert_eq!(&after_read, b"## Section\n\nContent.\n\n");
    assert_eq!(write(after_read), original);
}

/// Tests round-trip: edited content has trailing newline stripped on write.
#[test]
fn round_trip_with_edit() {
    let original = b"fn foo() {\n    bar()\n}".to_vec();
    let _after_read = read(original);

    // User adds a line via editor (editor-provided content ends with \n).
    let edited = b"fn foo() {\n    bar()\n    baz()\n}\n".to_vec();
    assert_eq!(&write(edited), b"fn foo() {\n    bar()\n    baz()\n}");
}
