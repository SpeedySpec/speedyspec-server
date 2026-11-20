//! The Direct Store conformance suite: connecting, health and closing, as every Direct Store reports them.

use speedyspec_data::{DbError, DirectStore};

fn check<T>(store: &dyn DirectStore, name: &str, result: Result<T, DbError>) -> T {
    result.unwrap_or_else(|e| {
        panic!(
            "direct conformance check `{name}` failed on `{}`: {e}",
            store.engine()
        )
    })
}

fn ensure(store: &dyn DirectStore, name: &str, ok: bool, detail: impl FnOnce() -> String) {
    assert!(
        ok,
        "direct conformance check `{name}` failed on `{}`: {}",
        store.engine(),
        detail()
    );
}

/// Runs every Direct Store check against `store`.
pub fn direct_store_suite(store: &dyn DirectStore, expected_kind: &str) {
    ensure(store, "kind", store.kind() == expected_kind, || {
        store.kind().to_string()
    });
    check(store, "connect", store.connect());
    let health = check(store, "connect", store.health());
    ensure(store, "connect", health.ok, || health.detail.clone());
    check(store, "connect_twice", store.connect());
    check(store, "close", store.close());
    let health = check(store, "close", store.health());
    ensure(store, "close", !health.ok, || "healthy after close".into());
    check(store, "close_twice", store.close());
    check(store, "reconnect", store.connect());
    let health = check(store, "reconnect", store.health());
    ensure(store, "reconnect", health.ok, || health.detail.clone());
}
