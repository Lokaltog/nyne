use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;

use rstest::rstest;

use super::*;
use crate::router::{CachePolicy, Node};

fn entry(dir: &str, name: &str) -> InodeEntry { InodeEntry::new(PathBuf::from(dir), name.to_owned(), ROOT_INODE) }

#[rstest]
#[case::known_path_returns_inode("src", "main.rs", true)]
#[case::unknown_name_returns_none("src", "lib.rs", false)]
#[case::unknown_dir_returns_none("other", "main.rs", false)]
fn find_inode_resolves_paths(#[case] dir: &str, #[case] name: &str, #[case] expect_some: bool) {
    let map = InodeMap::new();
    let allocated = map.allocate(entry("src", "main.rs"));
    let found = map.find_inode(Path::new(dir), name);
    if expect_some {
        assert_eq!(found, Some(allocated));
    } else {
        assert_eq!(found, None);
    }
}

#[test]
fn update_moves_reverse_index() {
    let map = InodeMap::new();
    let ino = map.allocate(entry("src", "old.rs"));
    assert_eq!(map.find_inode(Path::new("src"), "old.rs"), Some(ino));

    map.update(ino, PathBuf::from("dst"), "new.rs".to_owned(), ROOT_INODE);

    assert_eq!(map.find_inode(Path::new("src"), "old.rs"), None);
    assert_eq!(map.find_inode(Path::new("dst"), "new.rs"), Some(ino));
}

#[test]
fn get_roundtrips() {
    let map = InodeMap::new();
    let ino = map.allocate(entry("src", "main.rs"));
    let e = map.get(ino).unwrap();
    assert_eq!(e.dir_path, Path::new("src"));
    assert_eq!(e.name, "main.rs");
}

#[test]
fn multiple_entries_distinct() {
    let map = InodeMap::new();
    let a = map.allocate(entry("src", "a.rs"));
    let b = map.allocate(entry("src", "b.rs"));
    assert_ne!(a, b);
    assert_eq!(map.find_inode(Path::new("src"), "a.rs"), Some(a));
    assert_eq!(map.find_inode(Path::new("src"), "b.rs"), Some(b));
}

/// Bound-node lifecycle: bind → live for the TTL → expire (lazily on
/// access) → optionally re-armed by `touch`.
#[rstest]
#[case::unbound(None, None, false, true)]
#[case::bound_within_ttl(Some(Duration::from_secs(60)), None, false, false)]
#[case::bound_after_expiry(Some(Duration::from_millis(10)), Some(Duration::from_millis(30)), false, true)]
#[case::touch_extends_past_expiry(Some(Duration::from_millis(50)), Some(Duration::from_millis(30)), true, false)]
fn bound_node_lifecycle(
    #[case] bind_ttl: Option<Duration>,
    #[case] sleep_after_bind: Option<Duration>,
    #[case] touch_after_sleep: bool,
    #[case] expect_none: bool,
) {
    let map = InodeMap::new();
    let ino = map.allocate(entry("a", "b"));
    if let Some(ttl) = bind_ttl {
        let node = Node::file().with_cache_policy(CachePolicy::Ttl(ttl)).named("b");
        map.bind_node(ino, node);
    }
    if let Some(d) = sleep_after_bind {
        sleep(d);
        if touch_after_sleep {
            map.touch(ino);
            // Sleep again past the original TTL to prove `touch` actually
            // refreshed the deadline rather than just no-op'd.
            sleep(d);
        }
    }
    assert_eq!(map.bound_node(ino).is_none(), expect_none);
}

/// `touch` on an unbound inode is a no-op (must not panic and must not
/// retroactively mark the inode as bound).
#[test]
fn touch_is_noop_for_unbound_inode() {
    let map = InodeMap::new();
    let ino = map.allocate(entry("a", "b"));
    map.touch(ino);
    assert!(map.bound_node(ino).is_none());
}
