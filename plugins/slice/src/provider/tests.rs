use std::path::Path;
use std::sync::Mutex;

use nyne::router::{AffectedFiles, ReadContext, Readable, Writable, WriteContext};
use nyne::test_support::StubFs;
use rstest::rstest;

use super::*;

#[rstest]
#[case::single_line("file.rs:10", Some(("file.rs", "10")))]
#[case::range("file.rs:10-20", Some(("file.rs", "10-20")))]
#[case::multiple_colons("a:b:10", Some(("a:b", "10")))]
#[case::no_colon("file.rs", None)]
#[case::trailing_colon("file.rs:", None)]
#[case::leading_colon(":10", None)]
#[case::non_numeric("file.rs:something", None)]
#[case::digit_then_alpha("file.rs:3abc", None)]
#[case::double_dash("file.rs:3--5", None)]
#[case::todo_entry("plugins__analysis__src__engine__rules__todo_fixme.rs:3--markers-requires", None)]
fn parse_slice_suffix_cases(#[case] input: &str, #[case] expected: Option<(&str, &str)>) {
    assert_eq!(parse_slice_suffix(input), expected);
}

#[rstest]
#[case::single("42", 42, None)]
#[case::range("10-20", 10, Some(20))]
fn parse_range_valid(#[case] spec: &str, #[case] start: usize, #[case] end: Option<usize>) {
    let r = parse_range(spec).unwrap();
    assert_eq!(r.start, start);
    assert_eq!(r.end, end);
}

#[test]
fn parse_range_inverted_is_err() {
    assert!(parse_range("20-10").is_err());
}

#[rstest]
#[case::single_line(1, None, 5, 0..1)]
#[case::range(2, Some(4), 5, 1..4)]
#[case::end_clamped(3, Some(100), 5, 2..5)]
#[case::zero_start(0, None, 5, 0..0)]
#[case::single_last(5, None, 5, 4..5)]
fn line_range(
    #[case] start: usize,
    #[case] end: Option<usize>,
    #[case] line_count: usize,
    #[case] expected: std::ops::Range<usize>,
) {
    let spec = SliceSpec { start, end };
    assert_eq!(spec.line_range(line_count), expected);
}

/// Static readable that returns fixed content.
struct MockReadable(Vec<u8>);

impl Readable for MockReadable {
    fn read(&self, _ctx: &ReadContext<'_>) -> Result<Vec<u8>> { Ok(self.0.clone()) }
}

/// Writable that records the data it receives.
struct MockWritable(Mutex<Vec<u8>>);

impl Writable for MockWritable {
    fn write(&self, _ctx: &WriteContext<'_>, data: &[u8]) -> Result<AffectedFiles> {
        *self.0.lock().unwrap() = data.to_vec();
        Ok(vec![])
    }
}

fn make_splice(content: &str, start: usize, end: Option<usize>) -> (DefaultSplicingWritable, Arc<MockWritable>) {
    let writable = Arc::new(MockWritable(Mutex::new(vec![])));
    let splice = DefaultSplicingWritable {
        readable: Arc::new(MockReadable(content.as_bytes().to_vec())),
        writable: Arc::clone(&writable) as Arc<dyn Writable>,
        start,
        end,
    };
    (splice, writable)
}

fn do_write(splice: &DefaultSplicingWritable, data: &str) -> Result<AffectedFiles> {
    let stub = StubFs;
    let ctx = WriteContext {
        path: Path::new("test"),
        fs: &stub,
    };
    splice.write(&ctx, data.as_bytes())
}

fn written(mock: &MockWritable) -> String { String::from_utf8(mock.0.lock().unwrap().clone()).unwrap() }

#[rstest]
#[case::replace_single_line("aaa\nbbb\nccc\n", 2, None, "XXX", "aaa\nXXX\nccc\n")]
#[case::replace_range("aaa\nbbb\nccc\nddd\n", 2, Some(3), "XXX\nYYY", "aaa\nXXX\nYYY\nddd\n")]
#[case::replace_first_line("aaa\nbbb\nccc\n", 1, None, "XXX", "XXX\nbbb\nccc\n")]
#[case::replace_with_fewer_lines("aaa\nbbb\nccc\n", 1, Some(2), "XXX", "XXX\nccc\n")]
#[case::replace_with_more_lines("aaa\nbbb\nccc\n", 2, None, "XXX\nYYY", "aaa\nXXX\nYYY\nccc\n")]
fn default_splicing_writable(
    #[case] content: &str,
    #[case] start: usize,
    #[case] end: Option<usize>,
    #[case] new_data: &str,
    #[case] expected: &str,
) {
    let (splice, mock) = make_splice(content, start, end);
    do_write(&splice, new_data).unwrap();
    assert_eq!(written(&mock), expected);
}
