//! The per-worker cache: one file per identity, replaced when the file on disk changes.

use std::path::PathBuf;
use std::rc::Rc;

use speedyspec_wp_i18n::TranslationFile;
use speedyspec_wp_i18n_contracts::{
    FileIdentity, FileProbe, TranslationFileCache, TranslationFileReader,
};
use speedyspec_wp_i18n_files::{MoReader, StdFileProbe};
use speedyspec_wp_i18n_memory::MemoryTranslationFileCache;

fn id(size: u64, mtime_ns: i128) -> FileIdentity {
    FileIdentity {
        path: PathBuf::from("/l/de_DE.mo"),
        dev: 1,
        ino: 9,
        size,
        mtime_ns,
    }
}

#[test]
fn hit_on_same_identity() {
    let cache: MemoryTranslationFileCache<u32> = MemoryTranslationFileCache::default();
    cache.put(id(10, 1), Rc::new(7));
    assert_eq!(cache.get(&id(10, 1)).as_deref(), Some(&7));
}

#[test]
fn miss_after_size_or_mtime_change() {
    let cache: MemoryTranslationFileCache<u32> = MemoryTranslationFileCache::default();
    cache.put(id(10, 1), Rc::new(7));
    assert!(cache.get(&id(11, 1)).is_none());
    assert!(cache.get(&id(10, 2)).is_none());
}

#[test]
fn put_replaces_same_path() {
    let cache: MemoryTranslationFileCache<u32> = MemoryTranslationFileCache::default();
    cache.put(id(10, 1), Rc::new(7));
    cache.put(id(11, 2), Rc::new(8));
    assert_eq!(cache.len(), 1);
    assert_eq!(cache.get(&id(11, 2)).as_deref(), Some(&8));
}

#[test]
fn rewritten_file_is_reread() {
    let data =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../speedyspec-wp-i18n-files/tests/data");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.mo");
    std::fs::copy(data.join("simple.mo"), &path).unwrap();
    let probe = StdFileProbe;
    let cache: MemoryTranslationFileCache<TranslationFile> = MemoryTranslationFileCache::default();
    let load = |cache: &MemoryTranslationFileCache<TranslationFile>| {
        let identity = probe.identity(&path).unwrap();
        cache.get(&identity).unwrap_or_else(|| {
            let file = Rc::new(TranslationFile::new(
                identity.clone(),
                MoReader.read(&path).unwrap(),
            ));
            cache.put(identity, file.clone());
            file
        })
    };
    let old = load(&cache);
    assert!(old.translate(b"baba").is_some());
    // Rewritten in place, as WordPress's upgrader writes language packs.
    std::fs::write(&path, std::fs::read(data.join("plural.mo")).unwrap()).unwrap();
    let new = load(&cache);
    assert!(new.translate(b"baba").is_none());
    assert!(new.entries().count() > 0);
    // The old version still reads: it owns its bytes.
    assert_eq!(old.translate(b"baba"), Some(&b"dyado"[..]));
    assert_eq!(cache.len(), 1);
}
