use std::path::{Path, PathBuf};

use speedyspec_cache::CachingStore;
use speedyspec_cache_conformance::caching_store_suite;
use speedyspec_cache_file::{FileCache, entry_path};
use speedyspec_data::DirectStore;
use speedyspec_data_file::FileStore;
use speedyspec_data_memory::MemoryDirectStore;

fn store(root: &Path) -> FileStore {
    let store = FileStore::new("files", root.to_path_buf());
    store.connect().unwrap();
    store
}

fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        out.push(path.clone());
        if path.is_dir() {
            out.extend(files(&path));
        }
    }
    out
}

#[test]
fn file_cache_conformance() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    caching_store_suite(&FileCache::new(&store, "object-cache").unwrap());
}

#[test]
fn hostile_names_stay_inside() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let cache = FileCache::new(&store, "object-cache").unwrap();
    let long = "k".repeat(1024);
    for (group, key) in [("../..", "/etc/passwd\0"), ("g", long.as_str())] {
        cache.set(group, key, b"v", None).unwrap();
        assert_eq!(
            cache.get(group, key).unwrap().as_deref(),
            Some(b"v".as_slice()),
            "{group:?}"
        );
    }
    let inside = dir.path().join("object-cache");
    for f in files(dir.path()) {
        assert!(
            f == inside || f.starts_with(&inside),
            "{} is outside",
            f.display()
        );
    }
}

#[test]
fn layout_is_stable() {
    assert_eq!(
        entry_path("c", "posts", "1"),
        "c/g-a44f1b975171163546d46ba5ca80652f/e1/e468c91b781b41c4f672fb2980e9393d446ebcd97c638009224ff509f0288f"
    );
}

#[test]
fn flush_group_leaves_others() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let cache = FileCache::new(&store, "object-cache").unwrap();
    cache.set("posts", "1", b"p", None).unwrap();
    cache.set("users", "1", b"u", None).unwrap();
    cache.flush_group("posts").unwrap();
    assert_eq!(cache.get("posts", "1").unwrap(), None);
    assert_eq!(
        cache.get("users", "1").unwrap().as_deref(),
        Some(b"u".as_slice())
    );
    let names: Vec<String> = std::fs::read_dir(dir.path().join("object-cache"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().all(|n| !n.starts_with(".trash-")), "{names:?}");
}

#[test]
fn rejects_non_file_store() {
    let store = MemoryDirectStore::new("memory");
    assert!(FileCache::new(&store, "object-cache").is_err());
}

#[test]
fn concurrent_increments() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    FileCache::new(&store(&root), "object-cache")
        .unwrap()
        .set("g", "n", b"0", None)
        .unwrap();
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let root = root.clone();
            std::thread::spawn(move || {
                let store = store(&root);
                let cache = FileCache::new(&store, "object-cache").unwrap();
                for _ in 0..200 {
                    cache.increment("g", "n", 1).unwrap();
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let cache = FileCache::new(&store(&root), "object-cache").unwrap();
    assert_eq!(
        cache.get("g", "n").unwrap().as_deref(),
        Some(b"400".as_slice())
    );
}

#[test]
fn writes_do_not_sync_each_entry() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let cache = FileCache::new(&store, "object-cache").unwrap();
    let start = std::time::Instant::now();
    for i in 0..300 {
        cache.set("g", &i.to_string(), b"v", None).unwrap();
    }
    assert!(
        start.elapsed() < std::time::Duration::from_millis(750),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn purge_expired_removes_unread_entries() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let cache = FileCache::new(&store, "object-cache").unwrap();
    for k in ["a", "b", "c"] {
        cache
            .set("g", k, b"v", Some(std::time::Duration::from_millis(10)))
            .unwrap();
    }
    cache.set("g", "kept", b"v", None).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(30));
    assert_eq!(cache.purge_expired().unwrap(), 3);
    assert_eq!(
        cache.get("g", "kept").unwrap().as_deref(),
        Some(b"v".as_slice())
    );
}

#[test]
fn flush_removes_leftover_trash() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let cache = FileCache::new(&store, "object-cache").unwrap();
    cache.set("g", "k", b"v", None).unwrap();
    std::fs::create_dir_all(dir.path().join("object-cache/.trash-1-2/x")).unwrap();
    cache.flush().unwrap();
    let left: Vec<_> = std::fs::read_dir(dir.path().join("object-cache"))
        .unwrap()
        .collect();
    assert!(left.is_empty(), "{left:?}");
}
