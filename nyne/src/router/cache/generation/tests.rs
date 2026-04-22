use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use rstest::rstest;

use super::*;

#[rstest]
fn get_returns_zero_for_unknown() {
    assert_eq!(GenerationMap::new().get(Path::new("unknown.rs")), 0);
}

#[rstest]
fn bump_increments_and_returns() {
    let map = GenerationMap::new();
    let path = Path::new("foo.rs");

    assert_eq!(map.bump(path), 1);
    assert_eq!(map.bump(path), 2);
    assert_eq!(map.bump(path), 3);
    assert_eq!(map.get(path), 3);
}

#[rstest]
fn bump_increments_all() {
    let map = GenerationMap::new();
    let paths = vec![PathBuf::from("a.rs"), PathBuf::from("b.rs"), PathBuf::from("c.rs")];

    for p in &paths {
        map.bump(p);
    }
    assert_eq!(map.get(&paths[0]), 1);
    assert_eq!(map.get(&paths[1]), 1);
    assert_eq!(map.get(&paths[2]), 1);

    // Bump again -- all should increment
    for p in &paths {
        map.bump(p);
    }
    assert_eq!(map.get(&paths[0]), 2);
    assert_eq!(map.get(&paths[1]), 2);
    assert_eq!(map.get(&paths[2]), 2);
}

#[rstest]
fn concurrent_reads_dont_block() {
    let map = Arc::new(GenerationMap::new());
    let path = PathBuf::from("shared.rs");
    map.bump(&path);

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let map = Arc::clone(&map);
            let path = path.clone();
            thread::spawn(move || {
                for _ in 0..100 {
                    assert!(map.get(&path) >= 1);
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("thread panicked");
    }
}
