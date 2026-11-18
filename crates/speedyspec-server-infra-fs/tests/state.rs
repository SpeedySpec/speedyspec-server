use std::time::{Duration, Instant};

use speedyspec_server_domain::{ReleaseState, StateStore};
use speedyspec_server_infra_fs::FsStateStore;

fn store() -> (tempfile::TempDir, FsStateStore) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("data")).unwrap();
    std::fs::create_dir_all(dir.path().join("run")).unwrap();
    let store = FsStateStore::new(&dir.path().join("data"), &dir.path().join("run"));
    (dir, store)
}

fn state(revision: u64) -> ReleaseState {
    ReleaseState {
        revision,
        ..ReleaseState::default()
    }
}

#[test]
fn missing_state_is_default() {
    let (_dir, store) = store();
    assert_eq!(store.load().unwrap(), ReleaseState::default());
}

#[test]
fn save_then_load() {
    let (_dir, store) = store();
    store.save(&state(4)).unwrap();
    assert_eq!(store.load().unwrap().revision, 4);
}

#[test]
fn leftover_tmp_is_ignored() {
    let (dir, store) = store();
    store.save(&state(5)).unwrap();
    std::fs::write(dir.path().join("data/state.json.tmp"), "{garbage").unwrap();
    assert_eq!(store.load().unwrap().revision, 5);
}

#[test]
fn save_is_atomic_on_replace() {
    let (dir, store) = store();
    store.save(&state(1)).unwrap();
    store.save(&state(2)).unwrap();
    let text = std::fs::read_to_string(dir.path().join("data/state.json")).unwrap();
    assert_eq!(
        serde_json::from_str::<ReleaseState>(&text)
            .unwrap()
            .revision,
        2
    );
    assert!(!dir.path().join("data/state.json.tmp").exists());
}

#[test]
fn corrupt_state_is_an_error_naming_the_file() {
    let (dir, store) = store();
    std::fs::write(dir.path().join("data/state.json"), "{garbage").unwrap();
    let err = store.load().unwrap_err();
    assert!(err.to_string().contains("state.json"), "{err}");
}

#[test]
fn save_takes_the_lock() {
    let (dir, store) = store();
    let lock = std::fs::File::create(dir.path().join("run/state.lock")).unwrap();
    lock.lock().unwrap();
    let holder = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        drop(lock);
    });
    let started = Instant::now();
    store.save(&state(1)).unwrap();
    assert!(
        started.elapsed() >= Duration::from_millis(250),
        "{:?}",
        started.elapsed()
    );
    holder.join().unwrap();
}
