use super::*;

fn vpath(s: &str) -> VfsPath { VfsPath::new(s).unwrap() }

#[test]
fn file_generations_starts_at_zero() {
    let fg = FileGenerations::new();
    assert_eq!(fg.get(&vpath("a.rs")), 0);
}

#[test]
fn file_generations_bump_increments() {
    let fg = FileGenerations::new();
    let p = vpath("a.rs");
    assert_eq!(fg.bump(&p), 1);
    assert_eq!(fg.bump(&p), 2);
    assert_eq!(fg.get(&p), 2);
}

#[test]
fn file_generations_independent_per_file() {
    let fg = FileGenerations::new();
    fg.bump(&vpath("a.rs"));
    fg.bump(&vpath("a.rs"));
    fg.bump(&vpath("b.rs"));
    assert_eq!(fg.get(&vpath("a.rs")), 2);
    assert_eq!(fg.get(&vpath("b.rs")), 1);
}

fn make_cache() -> ContentCache { ContentCache::new(Arc::new(FileGenerations::new())) }

#[test]
fn get_returns_fresh_entry() {
    let cache = make_cache();
    let sf = vpath("src/lib.rs");
    cache.insert(1, b"hello".to_vec(), ProviderId::new("test"), Some(&sf));
    assert!(cache.get(1).is_some());
}

#[test]
fn get_evicts_stale_entry() {
    let cache = make_cache();
    let sf = vpath("src/lib.rs");
    cache.insert(1, b"hello".to_vec(), ProviderId::new("test"), Some(&sf));
    cache.file_generations.bump(&sf);
    assert!(cache.get(1).is_none(), "stale entry should be evicted");
}

#[test]
fn get_size_evicts_stale_entry() {
    let cache = make_cache();
    let sf = vpath("src/lib.rs");
    cache.insert(1, b"hello".to_vec(), ProviderId::new("test"), Some(&sf));
    cache.file_generations.bump(&sf);
    assert!(cache.get_size(1).is_none(), "stale entry should be evicted");
}

#[test]
fn entry_without_source_is_never_stale() {
    let cache = make_cache();
    cache.insert(1, b"hello".to_vec(), ProviderId::new("test"), None);
    // Bump some unrelated file — should not affect this entry.
    cache.file_generations.bump(&vpath("unrelated.rs"));
    assert!(cache.get(1).is_some());
}

#[test]
fn reinsert_after_bump_is_fresh() {
    let cache = make_cache();
    let sf = vpath("src/lib.rs");
    cache.insert(1, b"old".to_vec(), ProviderId::new("test"), Some(&sf));
    cache.file_generations.bump(&sf);
    assert!(cache.get(1).is_none());
    // Re-insert at current generation — should be fresh.
    cache.insert(1, b"new".to_vec(), ProviderId::new("test"), Some(&sf));
    assert!(cache.get(1).is_some());
}
