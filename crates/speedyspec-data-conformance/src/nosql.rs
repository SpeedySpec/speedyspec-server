//! The NoSQL Store conformance suite: the behaviour every key-value engine shares, Valkey's included.

use std::ops::Bound;
use std::thread::sleep;
use std::time::Duration;

use speedyspec_data::{DbError, NoSqlStore};

fn check<T>(store: &dyn NoSqlStore, name: &str, result: Result<T, DbError>) -> T {
    result.unwrap_or_else(|e| {
        panic!(
            "nosql conformance check `{name}` failed on `{}`: {e}",
            store.engine()
        )
    })
}

fn ensure(store: &dyn NoSqlStore, name: &str, ok: bool, detail: impl FnOnce() -> String) {
    assert!(
        ok,
        "nosql conformance check `{name}` failed on `{}`: {}",
        store.engine(),
        detail()
    );
}

fn engine_code<T: std::fmt::Debug>(result: &Result<T, DbError>) -> Option<&str> {
    match result {
        Err(DbError::Engine { code: Some(c), .. }) => Some(c.as_str()),
        _ => None,
    }
}

/// Runs every NoSQL Store check against `store` in `keyspace` (and `<keyspace>-other`), clearing both first.
pub fn nosql_store_suite(store: &dyn NoSqlStore, keyspace: &str) {
    let k = keyspace;
    let other = format!("{keyspace}-other");
    check(store, "setup", store.clear(k));
    check(store, "setup", store.clear(&other));

    // 1. missing
    ensure(
        store,
        "missing",
        check(store, "missing", store.get(k, b"nope")).is_none(),
        || "a value".into(),
    );

    // 2. roundtrip
    check(store, "roundtrip", store.put(k, b"a", b"1", None));
    let got = check(store, "roundtrip", store.get(k, b"a"));
    ensure(
        store,
        "roundtrip",
        got.as_deref() == Some(b"1".as_slice()),
        || format!("{got:?}"),
    );
    check(store, "roundtrip", store.put(k, b"a", b"2", None));
    let got = check(store, "roundtrip", store.get(k, b"a"));
    ensure(
        store,
        "roundtrip",
        got.as_deref() == Some(b"2".as_slice()),
        || format!("{got:?}"),
    );

    // 3. binary
    let binary: &[u8] = &[0x00, 0xFF, b'/'];
    check(store, "binary", store.put(k, binary, &[0x00, 0xFE], None));
    let got = check(store, "binary", store.get(k, binary));
    ensure(
        store,
        "binary",
        got.as_deref() == Some([0x00u8, 0xFE].as_slice()),
        || format!("{got:?}"),
    );
    check(store, "binary", store.put(k, b"empty", b"", None));
    let got = check(store, "binary", store.get(k, b"empty"));
    ensure(store, "binary", got == Some(Vec::new()), || {
        format!("{got:?}")
    });

    // 4. get_many_order
    let many = check(
        store,
        "get_many_order",
        store.get_many(k, &[b"a", b"nope", binary]),
    );
    ensure(
        store,
        "get_many_order",
        many == vec![Some(b"2".to_vec()), None, Some(vec![0x00, 0xFE])],
        || format!("{many:?}"),
    );
    let none = check(store, "get_many_order", store.get_many(k, &[]));
    ensure(store, "get_many_order", none.is_empty(), || {
        format!("{none:?}")
    });

    // 5. delete
    ensure(
        store,
        "delete",
        check(store, "delete", store.delete(k, b"a")),
        || "first delete was false".into(),
    );
    ensure(
        store,
        "delete",
        !check(store, "delete", store.delete(k, b"a")),
        || "second delete was true".into(),
    );
    ensure(
        store,
        "delete",
        check(store, "delete", store.get(k, b"a")).is_none(),
        || "still there".into(),
    );

    // 6. keyspaces_isolated
    check(
        store,
        "keyspaces_isolated",
        store.put(&other, b"a", b"o", None),
    );
    ensure(
        store,
        "keyspaces_isolated",
        check(store, "keyspaces_isolated", store.get(k, b"a")).is_none(),
        || "the other keyspace's key is visible".into(),
    );
    check(store, "keyspaces_isolated", store.clear(k));
    ensure(
        store,
        "keyspaces_isolated",
        check(store, "keyspaces_isolated", store.get(&other, b"a")).is_some(),
        || "clear removed the other keyspace's key".into(),
    );

    // 7. increment
    let n = check(store, "increment", store.increment(k, b"i", 5));
    ensure(store, "increment", n == 5, || format!("{n}"));
    let n = check(store, "increment", store.increment(k, b"i", -7));
    ensure(store, "increment", n == -2, || format!("{n}"));
    let got = check(store, "increment", store.get(k, b"i"));
    ensure(
        store,
        "increment",
        got.as_deref() == Some(b"-2".as_slice()),
        || format!("{got:?}"),
    );
    check(store, "increment", store.put(k, b"c", b"10", None));
    let n = check(store, "increment", store.increment(k, b"c", 5));
    ensure(store, "increment", n == 15, || format!("{n}"));

    // 8. increment_rejects
    for bad in [b"abc".as_slice(), b"05", b" 5", b""] {
        check(store, "increment_rejects", store.put(k, b"bad", bad, None));
        let result = store.increment(k, b"bad", 1);
        ensure(
            store,
            "increment_rejects",
            engine_code(&result) == Some("not-integer"),
            || format!("{:?} gave {result:?}", String::from_utf8_lossy(bad)),
        );
        let kept = check(store, "increment_rejects", store.get(k, b"bad"));
        ensure(
            store,
            "increment_rejects",
            kept.as_deref() == Some(bad),
            || format!("{kept:?}"),
        );
    }
    let max = i64::MAX.to_string();
    check(
        store,
        "increment_rejects",
        store.put(k, b"max", max.as_bytes(), None),
    );
    let result = store.increment(k, b"max", 1);
    ensure(
        store,
        "increment_rejects",
        engine_code(&result) == Some("overflow"),
        || format!("{result:?}"),
    );
    let kept = check(store, "increment_rejects", store.get(k, b"max"));
    ensure(
        store,
        "increment_rejects",
        kept.as_deref() == Some(max.as_bytes()),
        || format!("{kept:?}"),
    );

    // 8b. adjust_existing: atomic, never creates a key, keeps the TTL, floors when asked.
    let got = check(
        store,
        "adjust_existing",
        store.adjust_existing(k, b"absent", 3, None),
    );
    ensure(store, "adjust_existing", got.is_none(), || {
        format!("{got:?}")
    });
    let created = check(store, "adjust_existing", store.get(k, b"absent"));
    ensure(store, "adjust_existing", created.is_none(), || {
        format!("an absent key was created: {created:?}")
    });
    check(store, "adjust_existing", store.put(k, b"a", b"10", None));
    let got = check(
        store,
        "adjust_existing",
        store.adjust_existing(k, b"a", 5, None),
    );
    ensure(store, "adjust_existing", got == Some(15), || {
        format!("{got:?}")
    });
    let got = check(
        store,
        "adjust_existing",
        store.adjust_existing(k, b"a", -20, Some(0)),
    );
    ensure(store, "adjust_existing", got == Some(0), || {
        format!("{got:?}")
    });
    let stored = check(store, "adjust_existing", store.get(k, b"a"));
    ensure(
        store,
        "adjust_existing",
        stored.as_deref() == Some(b"0".as_slice()),
        || format!("{stored:?}"),
    );
    let got = check(
        store,
        "adjust_existing",
        store.adjust_existing(k, b"a", -3, None),
    );
    ensure(store, "adjust_existing", got == Some(-3), || {
        format!("{got:?}")
    });
    check(store, "adjust_existing", store.put(k, b"bad", b"abc", None));
    let result = store.adjust_existing(k, b"bad", 1, None);
    ensure(
        store,
        "adjust_existing",
        engine_code(&result) == Some("not-integer"),
        || format!("{result:?}"),
    );
    check(
        store,
        "adjust_existing",
        store.put(k, b"max", max.as_bytes(), None),
    );
    let result = store.adjust_existing(k, b"max", 1, None);
    ensure(
        store,
        "adjust_existing",
        engine_code(&result) == Some("overflow"),
        || format!("{result:?}"),
    );
    check(
        store,
        "adjust_existing",
        store.put(k, b"ttl", b"1", Some(Duration::from_millis(200))),
    );
    check(
        store,
        "adjust_existing",
        store.adjust_existing(k, b"ttl", 1, None),
    );
    std::thread::sleep(Duration::from_millis(300));
    let kept = check(store, "adjust_existing", store.get(k, b"ttl"));
    ensure(store, "adjust_existing", kept.is_none(), || {
        format!("the adjust dropped the TTL: {kept:?}")
    });

    // 9. expiry
    check(
        store,
        "expiry",
        store.put(k, b"t", b"x", Some(Duration::from_millis(200))),
    );
    ensure(
        store,
        "expiry",
        check(store, "expiry", store.get(k, b"t")).is_some(),
        || "gone at once".into(),
    );
    sleep(Duration::from_millis(400));
    ensure(
        store,
        "expiry",
        check(store, "expiry", store.get(k, b"t")).is_none(),
        || "still there".into(),
    );
    let many = check(store, "expiry", store.get_many(k, &[b"t"]));
    ensure(store, "expiry", many == vec![None], || format!("{many:?}"));
    if store.capabilities().ordered_ranges {
        let all = check(
            store,
            "expiry",
            store.range(k, Bound::Included(b"t"), Bound::Included(b"t"), None),
        );
        ensure(store, "expiry", all.is_empty(), || format!("{all:?}"));
    }
    check(store, "expiry", store.put(k, b"z", b"x", None));
    check(
        store,
        "expiry",
        store.put(k, b"z", b"x", Some(Duration::ZERO)),
    );
    ensure(
        store,
        "expiry",
        check(store, "expiry", store.get(k, b"z")).is_none(),
        || "a zero TTL kept it".into(),
    );
    check(
        store,
        "expiry",
        store.put(k, b"u", b"x", Some(Duration::from_micros(10))),
    );
    check(
        store,
        "expiry",
        store.put(k, b"n", b"5", Some(Duration::from_millis(300))),
    );
    let n = check(store, "expiry", store.increment(k, b"n", 1));
    ensure(store, "expiry", n == 6, || format!("{n}"));
    sleep(Duration::from_millis(500));
    ensure(
        store,
        "expiry",
        check(store, "expiry", store.get(k, b"n")).is_none(),
        || "an increment dropped the expiry".into(),
    );

    // 10. range
    for i in 1..=5u8 {
        check(store, "range", store.put(k, &[b'r', b'0' + i], &[i], None));
    }
    if store.capabilities().ordered_ranges {
        let got = check(
            store,
            "range",
            store.range(k, Bound::Included(b"r2"), Bound::Excluded(b"r5"), None),
        );
        let want: Vec<(Vec<u8>, Vec<u8>)> =
            (2..=4u8).map(|i| (vec![b'r', b'0' + i], vec![i])).collect();
        ensure(store, "range", got == want, || format!("{got:?}"));
        check(store, "range", store.clear(k));
        for i in 1..=5u8 {
            check(store, "range", store.put(k, &[b'r', b'0' + i], &[i], None));
        }
        let got = check(
            store,
            "range",
            store.range(k, Bound::Unbounded, Bound::Unbounded, Some(2)),
        );
        let want: Vec<(Vec<u8>, Vec<u8>)> =
            (1..=2u8).map(|i| (vec![b'r', b'0' + i], vec![i])).collect();
        ensure(store, "range", got == want, || format!("{got:?}"));
    } else {
        let result = store.range(k, Bound::Unbounded, Bound::Unbounded, None);
        ensure(
            store,
            "range",
            engine_code(&result) == Some("unsupported"),
            || format!("{result:?}"),
        );
    }

    // 11. put_if_absent
    ensure(
        store,
        "put_if_absent",
        check(
            store,
            "put_if_absent",
            store.put_if_absent(k, b"p", b"1", None),
        ),
        || "the first call wrote nothing".into(),
    );
    ensure(
        store,
        "put_if_absent",
        !check(
            store,
            "put_if_absent",
            store.put_if_absent(k, b"p", b"2", None),
        ),
        || "the second call wrote".into(),
    );
    let kept = check(store, "put_if_absent", store.get(k, b"p"));
    ensure(
        store,
        "put_if_absent",
        kept.as_deref() == Some(b"1".as_slice()),
        || format!("{kept:?}"),
    );
    ensure(
        store,
        "put_if_absent",
        check(
            store,
            "put_if_absent",
            store.put_if_absent(k, b"q", b"1", Some(Duration::from_millis(200))),
        ),
        || "the first call with a TTL wrote nothing".into(),
    );
    sleep(Duration::from_millis(400));
    ensure(
        store,
        "put_if_absent",
        check(
            store,
            "put_if_absent",
            store.put_if_absent(k, b"q", b"2", None),
        ),
        || "an expired key still counted as present".into(),
    );

    // 12. clear
    check(store, "clear", store.clear(k));
    ensure(
        store,
        "clear",
        check(store, "clear", store.get(k, b"p")).is_none(),
        || "a key survived".into(),
    );
    check(store, "clear", store.clear("speedyspec-never-used"));
    check(store, "clear", store.clear(&other));
}
