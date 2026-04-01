use std::path::PathBuf;

use super::*;

fn entry(dir: &str, name: &str) -> InodeEntry {
    InodeEntry {
        dir_path: PathBuf::from(dir),
        name: name.to_owned(),
        parent_inode: ROOT_INODE,
    }
}

#[test]
fn allocate_and_find() {
    let map = InodeMap::new();
    let ino = map.allocate(entry("src", "main.rs"));
    assert_eq!(map.find_inode(Path::new("src"), "main.rs"), Some(ino));
    assert_eq!(map.find_inode(Path::new("src"), "lib.rs"), None);
}

#[test]
fn find_returns_none_for_unknown() {
    let map = InodeMap::new();
    assert_eq!(map.find_inode(Path::new("src"), "main.rs"), None);
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
