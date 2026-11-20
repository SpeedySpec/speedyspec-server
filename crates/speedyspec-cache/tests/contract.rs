use speedyspec_cache::{CacheError, CachingStore};

#[allow(dead_code)]
fn takes(_: &dyn CachingStore) {}

#[test]
fn caching_store_is_object_safe() {
    let _f: fn(&dyn CachingStore) = takes;
}

#[test]
fn cache_error_display() {
    let backend = CacheError::Backend {
        cache: "sql:default".into(),
        message: "gone".into(),
    }
    .to_string();
    assert!(
        backend.contains("sql:default") && backend.contains("gone"),
        "{backend}"
    );
    let nan = CacheError::NotANumber {
        group: "g".into(),
        key: "k".into(),
    }
    .to_string();
    assert!(nan.contains('g') && nan.contains('k'), "{nan}");
}
