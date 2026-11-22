use std::cell::Cell;
use std::num::NonZeroUsize;
use std::rc::Rc;
use std::time::Duration;

use speedyspec_cache::{CacheError, CachingStore};
use speedyspec_cache_memory::MemoryCache;
use speedyspec_wp_media_contracts::{
    ContainerKind, MediaInfo, MediaReader, MediaSource, ProbeLimits, RawViews, ReadError,
};
use speedyspec_wp_media_isolate::CachedReader;
use speedyspec_wp_media_testkit as kit;

#[derive(Clone)]
struct Counting {
    calls: Rc<Cell<u32>>,
    fail: bool,
}

impl MediaReader for Counting {
    fn read(&self, _source: MediaSource, _limits: &ProbeLimits) -> Result<MediaInfo, ReadError> {
        self.calls.set(self.calls.get() + 1);
        if self.fail {
            return Err(ReadError::Unrecognized);
        }
        Ok(MediaInfo {
            container: ContainerKind::Wav,
            mime_type: Some("audio/wav".into()),
            duration: Some(Duration::from_secs(1)),
            bitrate: None,
            file_size: None,
            created_at: None,
            streams: vec![],
            tags: vec![],
            pictures: vec![],
            raw: RawViews::default(),
        })
    }
}

fn counting(fail: bool) -> (Counting, Rc<Cell<u32>>) {
    let calls = Rc::new(Cell::new(0));
    (
        Counting {
            calls: calls.clone(),
            fail,
        },
        calls,
    )
}

fn cached(
    inner: Counting,
    store: Rc<dyn CachingStore>,
    root: &std::path::Path,
) -> CachedReader<Counting> {
    CachedReader::new(
        inner,
        store,
        root.to_path_buf(),
        Duration::from_secs(3600),
        NonZeroUsize::new(8).unwrap(),
    )
}

fn path_of(file: &tempfile::NamedTempFile) -> MediaSource {
    MediaSource::Path(file.path().to_path_buf())
}

#[test]
fn second_read_hits_the_local_cache() {
    let file = kit::write_temp(&kit::wav_pcm16(0.1, 8000, 1), "wav");
    let (inner, calls) = counting(false);
    let reader = cached(
        inner,
        Rc::new(MemoryCache::new()),
        file.path().parent().unwrap(),
    );
    let first = reader
        .read(path_of(&file), &ProbeLimits::default())
        .unwrap();
    let second = reader
        .read(path_of(&file), &ProbeLimits::default())
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(calls.get(), 1);
}

#[test]
fn another_worker_hits_the_shared_store() {
    let file = kit::write_temp(&kit::wav_pcm16(0.1, 8000, 1), "wav");
    let store: Rc<dyn CachingStore> = Rc::new(MemoryCache::new());
    let (inner, calls) = counting(false);
    let root = file.path().parent().unwrap();
    cached(inner.clone(), store.clone(), root)
        .read(path_of(&file), &ProbeLimits::default())
        .unwrap();
    cached(inner, store, root)
        .read(path_of(&file), &ProbeLimits::default())
        .unwrap();
    assert_eq!(calls.get(), 1);
}

#[test]
fn changed_file_misses() {
    let file = kit::write_temp(&kit::wav_pcm16(0.1, 8000, 1), "wav");
    let (inner, calls) = counting(false);
    let reader = cached(
        inner,
        Rc::new(MemoryCache::new()),
        file.path().parent().unwrap(),
    );
    reader
        .read(path_of(&file), &ProbeLimits::default())
        .unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(file.path())
        .and_then(|mut f| {
            use std::io::Write;
            f.write_all(&[0])
        })
        .unwrap();
    reader
        .read(path_of(&file), &ProbeLimits::default())
        .unwrap();
    assert_eq!(calls.get(), 2);

    let c = std::ffi::CString::new(file.path().to_str().unwrap()).unwrap();
    let times = [
        libc::timespec {
            tv_sec: 1_000_000,
            tv_nsec: 0,
        },
        libc::timespec {
            tv_sec: 1_000_000,
            tv_nsec: 0,
        },
    ];
    assert_eq!(
        unsafe { libc::utimensat(libc::AT_FDCWD, c.as_ptr(), times.as_ptr(), 0) },
        0
    );
    reader
        .read(path_of(&file), &ProbeLimits::default())
        .unwrap();
    assert_eq!(calls.get(), 3);
}

#[test]
fn errors_are_not_cached() {
    let file = kit::write_temp(b"junk", "bin");
    let (inner, calls) = counting(true);
    let reader = cached(
        inner,
        Rc::new(MemoryCache::new()),
        file.path().parent().unwrap(),
    );
    assert!(
        reader
            .read(path_of(&file), &ProbeLimits::default())
            .is_err()
    );
    assert!(
        reader
            .read(path_of(&file), &ProbeLimits::default())
            .is_err()
    );
    assert_eq!(calls.get(), 2);
}

#[test]
fn fd_bypasses_the_cache() {
    let file = kit::write_temp(&kit::wav_pcm16(0.1, 8000, 1), "wav");
    let (inner, calls) = counting(false);
    let reader = cached(
        inner,
        Rc::new(MemoryCache::new()),
        file.path().parent().unwrap(),
    );
    for _ in 0..2 {
        let fd = std::fs::File::open(file.path()).unwrap().into();
        reader
            .read(MediaSource::Fd(fd), &ProbeLimits::default())
            .unwrap();
    }
    assert_eq!(calls.get(), 2);
}

struct Broken;

fn broken() -> CacheError {
    CacheError::Backend {
        cache: "broken".into(),
        message: "down".into(),
    }
}

impl CachingStore for Broken {
    fn name(&self) -> &str {
        "broken"
    }
    fn get(&self, _: &str, _: &str) -> Result<Option<Vec<u8>>, CacheError> {
        Err(broken())
    }
    fn get_many(&self, _: &str, _: &[&str]) -> Result<Vec<Option<Vec<u8>>>, CacheError> {
        Err(broken())
    }
    fn set(&self, _: &str, _: &str, _: &[u8], _: Option<Duration>) -> Result<(), CacheError> {
        Err(broken())
    }
    fn add(&self, _: &str, _: &str, _: &[u8], _: Option<Duration>) -> Result<bool, CacheError> {
        Err(broken())
    }
    fn replace(&self, _: &str, _: &str, _: &[u8], _: Option<Duration>) -> Result<bool, CacheError> {
        Err(broken())
    }
    fn delete(&self, _: &str, _: &str) -> Result<bool, CacheError> {
        Err(broken())
    }
    fn increment(&self, _: &str, _: &str, _: i64) -> Result<Option<i64>, CacheError> {
        Err(broken())
    }
    fn decrement(&self, _: &str, _: &str, _: i64) -> Result<Option<i64>, CacheError> {
        Err(broken())
    }
    fn flush(&self) -> Result<(), CacheError> {
        Err(broken())
    }
    fn flush_group(&self, _: &str) -> Result<(), CacheError> {
        Err(broken())
    }
}

#[test]
fn store_errors_do_not_fail_reads() {
    let file = kit::write_temp(&kit::wav_pcm16(0.1, 8000, 1), "wav");
    let (inner, calls) = counting(false);
    let reader = cached(inner, Rc::new(Broken), file.path().parent().unwrap());
    reader
        .read(path_of(&file), &ProbeLimits::default())
        .unwrap();
    reader
        .read(path_of(&file), &ProbeLimits::default())
        .unwrap();
    assert_eq!(
        calls.get(),
        1,
        "the local cache still serves the second read"
    );
    assert_eq!(reader.store_failures_logged(), 1);
}
