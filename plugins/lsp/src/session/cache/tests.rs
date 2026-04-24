use std::path::Path;
use std::thread;
use std::time::Duration;

use rstest::rstest;

use super::*;

/// Build a `CacheKey` from string components for testing.
fn key<'a>(path: &'a str, method: &'a str, line: u32, param: u32) -> CacheKey<'a> {
    CacheKey {
        path: Path::new(path),
        method,
        line,
        param,
    }
}
/// Default 60s-TTL cache used by every test that doesn't care about expiry.
fn new_cache() -> Cache { Cache::new(Duration::from_secs(60)) }

/// Tests that `get` returns the inserted value, or `None` when nothing was inserted.
#[rstest]
#[case::after_insert(true)]
#[case::empty_cache(false)]
fn basic_get(#[case] insert_first: bool) {
    let cache = new_cache();
    let k = key("/src/main.rs", "references", 10, 5);
    let expected = insert_first.then(|| {
        let v = vec!["alpha".to_owned(), "beta".to_owned()];
        cache.insert(&k, v.clone());
        v
    });
    let result: Option<Vec<String>> = cache.get(&k);
    assert_eq!(result, expected);
}

/// Tests that entries expire after the configured TTL.
#[rstest]
fn ttl_expiry() {
    let cache = Cache::new(Duration::from_millis(1));
    let k = key("/src/main.rs", "hover", 5, 3);
    cache.insert(&k, 42_i32);

    thread::sleep(Duration::from_millis(5));

    let result: Option<i32> = cache.get(&k);
    assert_eq!(result, None);
}

/// Tests that `invalidate_file` clears only that file's entries, while `clear` drops
/// everything. Two pre-populated entries (main, lib) + operation → expected clear state.
#[rstest]
#[case::invalidate_file_scopes_to_path(ClearOp::InvalidateFile("/src/main.rs"), true, false)]
#[case::clear_drops_everything(ClearOp::Clear, true, true)]
fn clear_and_invalidate(#[case] op: ClearOp, #[case] main_cleared: bool, #[case] lib_cleared: bool) {
    let cache = new_cache();
    let main_key = key("/src/main.rs", "references", 10, 5);
    let lib_key = key("/src/lib.rs", "references", 1, 0);
    cache.insert(&main_key, "main_ref".to_owned());
    cache.insert(&lib_key, "lib_ref".to_owned());

    match op {
        ClearOp::InvalidateFile(p) => cache.invalidate_file(Path::new(p)),
        ClearOp::Clear => cache.clear(),
    }

    let main_val: Option<String> = cache.get(&main_key);
    let lib_val: Option<String> = cache.get(&lib_key);
    assert_eq!(main_val.is_none(), main_cleared, "main cleared?");
    assert_eq!(lib_val.is_none(), lib_cleared, "lib cleared?");
}

enum ClearOp {
    InvalidateFile(&'static str),
    Clear,
}

/// Tests that `len` and `is_empty` reflect the number of cached entries.
#[rstest]
fn len_and_is_empty() {
    let cache = new_cache();
    assert!(cache.is_empty());
    assert_eq!(cache.len(), 0);

    cache.insert(&key("/x", "hover", 0, 0), true);
    assert!(!cache.is_empty());
    assert_eq!(cache.len(), 1);

    cache.insert(&key("/y", "hover", 0, 0), false);
    assert_eq!(cache.len(), 2);
}

/// Tests that requesting a cached value as a different type returns `None`.
#[rstest]
fn type_mismatch_returns_none() {
    let cache = new_cache();
    let k = key("/src/main.rs", "hover", 1, 0);
    cache.insert(&k, 42_i32);
    let result: Option<String> = cache.get(&k);
    assert_eq!(result, None);
}

/// Tests that `get_with_age` returns the cached value along with its age.
#[rstest]
fn get_with_age_returns_age() {
    let cache = new_cache();
    let k = key("/src/main.rs", "hover", 1, 0);
    cache.insert(&k, "value".to_owned());
    thread::sleep(Duration::from_millis(10));

    let (value, age): (String, Duration) = cache.get_with_age(&k).expect("should be cached");
    assert_eq!(value, "value");
    assert!(age >= Duration::from_millis(10));
}
