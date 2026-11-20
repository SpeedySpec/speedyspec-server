//! The conformance suite every Caching Store runs; a failing check panics with its name.

use std::time::Duration;

use speedyspec_cache::{CacheError, CachingStore};

fn ok<T>(name: &str, r: Result<T, CacheError>) -> T {
    r.unwrap_or_else(|e| panic!("cache check `{name}` failed: {e}"))
}

fn ensure(name: &str, cond: bool, detail: impl FnOnce() -> String) {
    assert!(cond, "cache check `{name}` failed: {}", detail());
    println!("ok: {name}");
}

/// Runs every check against `cache`, starting with `flush()`.
pub fn caching_store_suite(cache: &dyn CachingStore) {
    ok("flush", cache.flush());
    let get = |g: &str, k: &str| ok("get", cache.get(g, k));

    ensure("miss_is_none", get("g", "absent").is_none(), || {
        "a value".into()
    });

    ok("set_get_bytes", cache.set("g", "k", b"a\0\xff", None));
    ensure(
        "set_get_bytes",
        get("g", "k").as_deref() == Some(b"a\0\xff"),
        || format!("{:?}", get("g", "k")),
    );

    ok("set_overwrites", cache.set("g", "k", b"2", None));
    ensure(
        "set_overwrites",
        get("g", "k").as_deref() == Some(b"2"),
        || format!("{:?}", get("g", "k")),
    );

    let added = ok("add_only_if_absent", cache.add("g", "k", b"3", None));
    ensure(
        "add_only_if_absent",
        !added && get("g", "k").as_deref() == Some(b"2"),
        || format!("{added}"),
    );
    let added = ok("add_only_if_absent", cache.add("g", "k2", b"x", None));
    ensure("add_only_if_absent", added, || {
        "add of an absent key was false".into()
    });

    let replaced = ok(
        "replace_only_if_present",
        cache.replace("g", "absent", b"r", None),
    );
    ensure(
        "replace_only_if_present",
        !replaced && get("g", "absent").is_none(),
        || format!("{replaced}"),
    );
    let replaced = ok(
        "replace_only_if_present",
        cache.replace("g", "k", b"4", None),
    );
    ensure(
        "replace_only_if_present",
        replaced && get("g", "k").as_deref() == Some(b"4"),
        || format!("{replaced}"),
    );

    let same = ok(
        "replace_same_value_true",
        cache.replace("g", "k", b"4", None),
    );
    ensure("replace_same_value_true", same, || {
        "replacing with the same value was false".into()
    });

    let first = ok("delete_reports_presence", cache.delete("g", "k2"));
    let again = ok("delete_reports_presence", cache.delete("g", "k2"));
    ensure("delete_reports_presence", first && !again, || {
        format!("{first} {again}")
    });

    let many = ok(
        "get_many_in_order",
        cache.get_many("g", &["k", "absent", "k"]),
    );
    ensure(
        "get_many_in_order",
        many == vec![Some(b"4".to_vec()), None, Some(b"4".to_vec())],
        || format!("{many:?}"),
    );

    ok("groups_are_separate", cache.set("g2", "k", b"other", None));
    ensure(
        "groups_are_separate",
        get("g", "k").as_deref() == Some(b"4"),
        || format!("{:?}", get("g", "k")),
    );

    ok("flush_group_only_that_group", cache.flush_group("g2"));
    ensure(
        "flush_group_only_that_group",
        get("g2", "k").is_none() && get("g", "k").is_some(),
        || "both or neither".into(),
    );

    ok("increment_decimal", cache.set("g", "n", b"5", None));
    let inc = ok("increment_decimal", cache.increment("g", "n", 3));
    let absent = ok("increment_decimal", cache.increment("g", "absent", 1));
    ok("increment_decimal", cache.set("g", "x", b"x", None));
    let nan = cache.increment("g", "x", 1);
    ensure(
        "increment_decimal",
        inc == Some(8)
            && get("g", "n").as_deref() == Some(b"8")
            && absent.is_none()
            && matches!(nan, Err(CacheError::NotANumber { .. })),
        || format!("{inc:?} {absent:?} {nan:?}"),
    );

    ok("decrement_floors_at_zero", cache.set("g", "n", b"2", None));
    let dec = ok("decrement_floors_at_zero", cache.decrement("g", "n", 5));
    ensure(
        "decrement_floors_at_zero",
        dec == Some(0) && get("g", "n").as_deref() == Some(b"0"),
        || format!("{dec:?}"),
    );

    ok(
        "expiry_zero_is_expired",
        cache.set("g", "z", b"v", Some(Duration::ZERO)),
    );
    let added = ok("expiry_zero_is_expired", cache.add("g", "z", b"w", None));
    ensure(
        "expiry_zero_is_expired",
        added && get("g", "z").as_deref() == Some(b"w"),
        || format!("{added}"),
    );

    ok(
        "expiry_elapses",
        cache.set("g", "e", b"v", Some(Duration::from_secs(1))),
    );
    ensure("expiry_elapses", get("g", "e").is_some(), || {
        "not readable at once".into()
    });
    std::thread::sleep(Duration::from_millis(2100));
    let gone = get("g", "e").is_none();
    let deleted = ok("expiry_elapses", cache.delete("g", "e"));
    let replaced = ok("expiry_elapses", cache.replace("g", "e", b"r", None));
    let added = ok("expiry_elapses", cache.add("g", "e", b"a", None));
    ensure(
        "expiry_elapses",
        gone && !deleted && !replaced && added,
        || format!("{gone} {deleted} {replaced} {added}"),
    );

    let long = "k".repeat(300);
    ok("long_and_odd_keys", cache.set("g", &long, b"long", None));
    ok(
        "long_and_odd_keys",
        cache.set("g", &long[..299], b"prefix", None),
    );
    ok("long_and_odd_keys", cache.set("g", "é:\0x", b"odd", None));
    ok("long_and_odd_keys", cache.set("g", "1:5", b"colon", None));
    ok(
        "long_and_odd_keys",
        cache.set("", "k", b"empty group", None),
    );
    ensure(
        "long_and_odd_keys",
        get("g", &long).as_deref() == Some(b"long")
            && get("g", &long[..299]).as_deref() == Some(b"prefix")
            && get("g", "é:\0x").as_deref() == Some(b"odd")
            && get("g", "1:5").as_deref() == Some(b"colon")
            && get("", "k").as_deref() == Some(b"empty group"),
        || "a key was lost or merged".into(),
    );

    ok("empty_value", cache.set("g", "empty", b"", None));
    ensure(
        "empty_value",
        get("g", "empty").as_deref() == Some(b""),
        || format!("{:?}", get("g", "empty")),
    );

    ok("flush_all", cache.flush());
    ensure(
        "flush_all",
        get("g", "k").is_none() && get("", "k").is_none(),
        || "flush left entries".into(),
    );
}
