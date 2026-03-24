use std::path::Path;
use std::thread;
use std::time::Duration;

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

/// Tests that inserting a value and retrieving it returns the same value.
#[test]
fn insert_and_get() {
    let cache = LspCache::new(Duration::from_secs(60));
    let values = vec!["alpha".to_owned(), "beta".to_owned()];
    let k = key("/src/main.rs", "references", 10, 5);
    cache.insert(&k, values.clone());

    let result: Option<Vec<String>> = cache.get(&k);
    assert_eq!(result, Some(values));
}

/// Tests that querying an empty cache returns `None`.
#[test]
fn miss_on_empty() {
    let cache = LspCache::new(Duration::from_secs(60));
    let k = key("/src/main.rs", "references", 1, 0);
    let result: Option<Vec<String>> = cache.get(&k);
    assert_eq!(result, None);
}

/// Tests that entries expire after the configured TTL.
#[test]
fn ttl_expiry() {
    let cache = LspCache::new(Duration::from_millis(1));
    let k = key("/src/main.rs", "hover", 5, 3);
    cache.insert(&k, 42_i32);

    thread::sleep(Duration::from_millis(5));

    let result: Option<i32> = cache.get(&k);
    assert_eq!(result, None);
}

/// Tests that invalidating a file clears only that file's entries.
#[test]
fn invalidate_file() {
    let cache = LspCache::new(Duration::from_secs(60));
    let main_key = key("/src/main.rs", "references", 10, 5);
    let lib_key = key("/src/lib.rs", "references", 1, 0);
    cache.insert(&main_key, "main_ref".to_owned());
    cache.insert(&lib_key, "lib_ref".to_owned());

    cache.invalidate_file(Path::new("/src/main.rs"));

    let main_result: Option<String> = cache.get(&main_key);
    let lib_result: Option<String> = cache.get(&lib_key);
    assert_eq!(main_result, None);
    assert_eq!(lib_result, Some("lib_ref".to_owned()));
}

/// Tests that clearing the cache removes all entries.
#[test]
fn clear() {
    let cache = LspCache::new(Duration::from_secs(60));
    let k1 = key("/a", "hover", 0, 0);
    let k2 = key("/b", "hover", 0, 0);
    cache.insert(&k1, 1_i32);
    cache.insert(&k2, 2_i32);

    cache.clear();

    let a: Option<i32> = cache.get(&k1);
    let b: Option<i32> = cache.get(&k2);
    assert_eq!(a, None);
    assert_eq!(b, None);
}

/// Tests that `len` and `is_empty` reflect the number of cached entries.
#[test]
fn len_and_is_empty() {
    let cache = LspCache::new(Duration::from_secs(60));
    assert!(cache.is_empty());
    assert_eq!(cache.len(), 0);

    let k1 = key("/x", "hover", 0, 0);
    cache.insert(&k1, true);
    assert!(!cache.is_empty());
    assert_eq!(cache.len(), 1);

    let k2 = key("/y", "hover", 0, 0);
    cache.insert(&k2, false);
    assert_eq!(cache.len(), 2);
}

/// Tests that requesting a cached value as a different type returns `None`.
#[test]
fn type_mismatch_returns_none() {
    let cache = LspCache::new(Duration::from_secs(60));
    let k = key("/src/main.rs", "hover", 1, 0);
    cache.insert(&k, 42_i32);

    // Requesting as String when stored as i32 should return None.
    let result: Option<String> = cache.get(&k);
    assert_eq!(result, None);
}

/// Tests that `get_with_age` returns the cached value along with its age.
#[test]
fn get_with_age_returns_age() {
    let cache = LspCache::new(Duration::from_secs(60));
    let k = key("/src/main.rs", "hover", 1, 0);
    cache.insert(&k, "value".to_owned());

    thread::sleep(Duration::from_millis(10));

    let result: Option<(String, Duration)> = cache.get_with_age(&k);
    let (value, age) = result.expect("should be cached");
    assert_eq!(value, "value");
    assert!(age >= Duration::from_millis(10));
}
