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
    // Only the group locks stay: removing a lock file another process holds would split the lock.
    let left: Vec<_> = std::fs::read_dir(dir.path().join("object-cache"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .filter(|n| n != ".locks")
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

/// Runs `f` on another thread while this one holds `group`'s lock for 200 ms; how long `f` took.
fn waits_for_the_group_lock(
    root: &Path,
    group: &str,
    f: impl FnOnce(FileCache) + Send + 'static,
) -> std::time::Duration {
    use speedyspec_data_file::FileClient;
    let holder = store(root);
    let client = holder
        .as_any()
        .downcast_ref::<FileClient>()
        .unwrap()
        .clone();
    let lock = client
        .lock(&speedyspec_cache_file::lock_path("object-cache", group))
        .unwrap();
    let root = root.to_path_buf();
    let (started, running) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        let other = store(&root);
        let cache = FileCache::new(&other, "object-cache").unwrap();
        started.send(()).unwrap();
        let start = std::time::Instant::now();
        f(cache);
        start.elapsed()
    });
    running.recv().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    drop(lock);
    thread.join().unwrap()
}

/// `flush_group` takes the group's lock, and the lock lives outside the group's directory, so a holder in another
/// process keeps it across the flush (two `add`s of one key can never both succeed).
#[test]
fn flush_group_waits_for_the_group_lock() {
    let dir = tempfile::tempdir().unwrap();
    let cache = FileCache::new(&store(dir.path()), "object-cache").unwrap();
    cache.set("g", "k", b"v", None).unwrap();
    let took = waits_for_the_group_lock(dir.path(), "g", |c| c.flush_group("g").unwrap());
    assert!(took >= std::time::Duration::from_millis(100), "{took:?}");
    assert_eq!(cache.get("g", "k").unwrap(), None);
}

/// The sweep takes each group's lock, so it never removes an entry a locked writer has just replaced.
#[test]
fn purge_expired_waits_for_the_group_lock() {
    let dir = tempfile::tempdir().unwrap();
    let cache = FileCache::new(&store(dir.path()), "object-cache").unwrap();
    cache
        .set("g", "k", b"v", Some(std::time::Duration::from_millis(1)))
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let took = waits_for_the_group_lock(dir.path(), "g", |c| {
        assert_eq!(c.purge_expired().unwrap(), 1);
    });
    assert!(took >= std::time::Duration::from_millis(100), "{took:?}");
}

/// A read holds no lock, so it leaves an expired entry for a locked writer or the sweep to remove; it only answers
/// that the entry is gone.
#[test]
fn get_leaves_an_expired_entry_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let cache = FileCache::new(&store(dir.path()), "object-cache").unwrap();
    cache
        .set("g", "k", b"v", Some(std::time::Duration::from_millis(1)))
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    assert_eq!(cache.get("g", "k").unwrap(), None);
    let path = dir.path().join(entry_path("object-cache", "g", "k"));
    assert!(path.exists(), "the read removed {}", path.display());
    assert!(cache.add("g", "k", b"new", None).unwrap());
    assert_eq!(cache.get("g", "k").unwrap().as_deref(), Some(&b"new"[..]));
}
