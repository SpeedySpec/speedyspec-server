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

/// Final review I1: two pods saving at once (a deposed leader's late save and the new leader's) never leave a
/// `state.json` that does not parse, and neither save trips over the other's temp file.
#[test]
fn concurrent_writers_never_corrupt_state() {
    use speedyspec_server_domain::{ReleaseState, StateStore};
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    let writers: Vec<_> = (0..2)
        .map(|n| {
            let (data, run) = (data.clone(), dir.path().join(format!("run-{n}")));
            std::thread::spawn(move || {
                let store = speedyspec_server_infra_fs::FsStateStore::new(&data, &run);
                let mut state = ReleaseState::default();
                for revision in 0..150u64 {
                    state.revision = revision;
                    state.epoch = n;
                    for i in 0..40 {
                        state
                            .server
                            .failed
                            .insert(semver::Version::new(n, revision, i), "x".repeat(64));
                    }
                    store
                        .save(&state)
                        .expect("a save never fails because of another writer");
                }
            })
        })
        .collect();
    let reader = speedyspec_server_infra_fs::FsStateStore::new(&data, &dir.path().join("run-r"));
    for _ in 0..300 {
        let text = std::fs::read(data.join("state.json")).unwrap_or_default();
        if !text.is_empty() {
            assert!(
                serde_json::from_slice::<ReleaseState>(&text).is_ok(),
                "state.json does not parse"
            );
        }
        let _ = reader.load();
    }
    for writer in writers {
        writer.join().unwrap();
    }
    reader.load().unwrap();
}

/// Final review I2: a torn `state.json` read keeps the last good copy, and is logged once.
#[test]
fn torn_state_read_keeps_last_good_copy() {
    use speedyspec_server_domain::{ReleaseState, StateStore};
    let dir = tempfile::tempdir().unwrap();
    let store = speedyspec_server_infra_fs::FsStateStore::new(dir.path(), &dir.path().join("run"));
    let mut state = ReleaseState::default();
    state.revision = 7;
    store.save(&state).unwrap();
    assert_eq!(store.load().unwrap().revision, 7);
    let path = dir.path().join("state.json");
    let bytes = std::fs::read(&path).unwrap();
    std::fs::write(&path, &bytes[..bytes.len() / 2]).unwrap();
    for _ in 0..3 {
        assert_eq!(store.load().unwrap().revision, 7, "the last good copy");
    }
    let fresh = speedyspec_server_infra_fs::FsStateStore::new(dir.path(), &dir.path().join("run"));
    assert!(
        fresh.load().is_err(),
        "a reader with no good copy yet reports the error"
    );
}
