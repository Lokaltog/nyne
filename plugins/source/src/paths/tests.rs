use super::*;

#[test]
fn default_uses_canonical_names() {
    let paths = SourcePaths::default();
    assert_eq!(paths.symbols_dir(), "symbols");
    assert_eq!(paths.at_line(42), "symbols/at-line/42");
}

#[test]
fn custom_config_propagates() {
    let dirs = VfsDirs {
        symbols: "sym".into(),
        at_line: "line".into(),
        ..VfsDirs::default()
    };
    let paths = SourcePaths::from_vfs(&dirs);
    assert_eq!(paths.symbols_dir(), "sym");
    assert_eq!(paths.at_line(1), "sym/line/1");
}

#[test]
fn at_line_formats_large_numbers() {
    let paths = SourcePaths::default();
    assert_eq!(paths.at_line(99999), "symbols/at-line/99999");
}
