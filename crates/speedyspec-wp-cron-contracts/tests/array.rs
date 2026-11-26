use speedyspec_hooks::{Key, Value};
use speedyspec_wp_cron_contracts::{CronArray, EventData, EventKey, ScheduledEvent};

fn s(v: &str) -> Value {
    Value::Str(v.into())
}

fn arr(items: Vec<(Key, Value)>) -> Value {
    Value::Array(items)
}

fn key(k: &str) -> EventKey {
    EventKey::parse(k).unwrap()
}

const K1: &str = "40cd750bba9870f18aada2478b24840a";
const K2: &str = "46a9951db4f3bfc932d84a13b4c11261";

fn single(args: Value) -> Value {
    arr(vec![
        (Key::Str("schedule".into()), Value::Bool(false)),
        (Key::Str("args".into()), args),
    ])
}

fn sample() -> Value {
    arr(vec![
        (
            Key::Int(100),
            arr(vec![(
                Key::Str("hook_a".into()),
                arr(vec![(Key::Str(K1.into()), single(arr(vec![])))]),
            )]),
        ),
        (Key::Str("junk".into()), s("kept")),
        (
            Key::Int(200),
            arr(vec![
                (Key::Str("not_an_array".into()), s("x")),
                (
                    Key::Str("hook_b".into()),
                    arr(vec![(
                        Key::Str(K2.into()),
                        single(arr(vec![(Key::Int(0), s("a"))])),
                    )]),
                ),
            ]),
        ),
    ])
}

#[test]
fn round_trip_keeps_order_and_junk() {
    let v = sample();
    assert_eq!(CronArray::from_value(v.clone()).to_value(), v);
    assert_eq!(CronArray::from_value(s("nonsense")).to_value(), arr(vec![]));
}

#[test]
fn events_are_read_in_order() {
    let events = CronArray::from_value(sample()).events();
    let seen: Vec<(i64, &str)> = events
        .iter()
        .map(|e| (e.timestamp, e.hook.as_str()))
        .collect();
    assert_eq!(seen, vec![(100, "hook_a"), (200, "hook_b")]);
    assert_eq!(events[1].data.schedule, None);
    assert_eq!(events[1].data.args, arr(vec![(Key::Int(0), s("a"))]));
}

#[test]
fn remove_prunes_empty_levels() {
    let mut cron = CronArray::from_value(sample());
    cron.remove(100, "hook_a", &key(K1));
    assert!(!cron.contains(100, "hook_a", &key(K1)));
    let keys: Vec<Key> = match cron.to_value() {
        Value::Array(items) => items.into_iter().map(|(k, _)| k).collect(),
        _ => unreachable!(),
    };
    assert_eq!(keys, vec![Key::Str("junk".into()), Key::Int(200)]);
    // A hook with another event keeps its level.
    cron.remove(200, "hook_b", &key(K2));
    assert_eq!(cron.events().len(), 0);
    // The junk hook entry keeps timestamp 200 alive, as WordPress's empty() checks would.
    assert!(matches!(cron.to_value(), Value::Array(items) if items.len() == 2));
}

#[test]
fn insert_builds_wordpress_shapes() {
    let mut cron = CronArray::from_value(arr(vec![]));
    cron.insert(ScheduledEvent {
        hook: "h".into(),
        timestamp: 300,
        key: key(K1),
        data: EventData {
            schedule: Some("hourly".into()),
            interval: Some(3600),
            args: arr(vec![]),
        },
    });
    cron.insert(ScheduledEvent {
        hook: "123".into(),
        timestamp: 300,
        key: key(K2),
        data: EventData {
            schedule: None,
            interval: None,
            args: arr(vec![]),
        },
    });
    let expected = arr(vec![(
        Key::Int(300),
        arr(vec![
            (
                Key::Str("h".into()),
                arr(vec![(
                    Key::Str(K1.into()),
                    arr(vec![
                        (Key::Str("schedule".into()), s("hourly")),
                        (Key::Str("args".into()), arr(vec![])),
                        (Key::Str("interval".into()), Value::Int(3600)),
                    ]),
                )]),
            ),
            // A numeric hook name is an integer key, as PHP makes it.
            (
                Key::Int(123),
                arr(vec![(Key::Str(K2.into()), single(arr(vec![])))]),
            ),
        ]),
    )]);
    assert_eq!(cron.to_value(), expected);
    assert!(cron.contains(300, "123", &key(K2)));
}

fn key_sorted(keys: Vec<Key>) -> Vec<Key> {
    let mut cron = CronArray::from_value(arr(keys.into_iter().map(|k| (k, arr(vec![]))).collect()));
    cron.key_sort();
    match cron.to_value() {
        Value::Array(items) => items.into_iter().map(|(k, _)| k).collect(),
        _ => unreachable!(),
    }
}

fn str_key(k: &str) -> Key {
    Key::Str(k.into())
}

#[test]
fn key_sort_matches_ksort_on_timestamps() {
    // PHP 8.5.12: ksort($a) on these keys gives
    // [-1,"9.9",10,"1e1","010","10.25","10.5"," 11","11 ","ABC","abc"]: numeric strings compare by value (equal
    // values keep their order), and WordPress 7.1.5's strnatcasecmp put "10.5" before "10.25".
    let sorted = key_sorted(vec![
        str_key("10.5"),
        str_key("10.25"),
        Key::Int(10),
        str_key("9.9"),
        str_key(" 11"),
        str_key("1e1"),
        str_key("010"),
        Key::Int(-1),
        str_key("abc"),
        str_key("ABC"),
        str_key("11 "),
    ]);
    assert_eq!(
        sorted,
        vec![
            Key::Int(-1),
            str_key("9.9"),
            Key::Int(10),
            str_key("1e1"),
            str_key("010"),
            str_key("10.25"),
            str_key("10.5"),
            str_key(" 11"),
            str_key("11 "),
            str_key("ABC"),
            str_key("abc"),
        ]
    );
}

#[test]
fn key_sort_matches_ksort_past_the_integer_range() {
    // PHP 8.5.12: ksort($a) on these keys gives ["-1e999",PHP_INT_MIN,PHP_INT_MAX,"99999999999999999998",
    // "99999999999999999999","1e999","x"].
    let sorted = key_sorted(vec![
        str_key("99999999999999999999"),
        str_key("99999999999999999998"),
        Key::Int(i64::MAX),
        str_key("1e999"),
        str_key("-1e999"),
        Key::Int(i64::MIN),
        str_key("x"),
    ]);
    assert_eq!(
        sorted,
        vec![
            str_key("-1e999"),
            Key::Int(i64::MIN),
            Key::Int(i64::MAX),
            str_key("99999999999999999998"),
            str_key("99999999999999999999"),
            str_key("1e999"),
            str_key("x"),
        ]
    );
}

#[test]
fn key_sort_places_junk_keys_as_php_does() {
    // Non-numeric keys beside timestamps make PHP's comparisons intransitive ("10a" < 3 < 42 < "9x" < "10a"), so
    // the order is zend_sort()'s own. PHP 8.5.12's ksort($a) on these 30 keys (its quicksort, not its insertion
    // sort) gives the order below.
    let sorted = key_sorted(vec![
        Key::Int(1700003600),
        str_key("next"),
        Key::Int(1700000000),
        str_key("9x"),
        Key::Int(1700007200),
        str_key(" 1700000500"),
        str_key("1700000100.5"),
        Key::Int(100),
        str_key("1e9"),
        str_key("abc"),
        Key::Int(1700001000),
        Key::Int(7),
        str_key("10a"),
        Key::Int(5),
        str_key("x9"),
        Key::Int(1700000001),
        Key::Int(1700000002),
        Key::Int(42),
        str_key("Z"),
        str_key("a"),
        Key::Int(1700009999),
        Key::Int(3),
        Key::Int(300),
        Key::Int(1700005000),
        str_key("0x1A"),
        str_key(""),
        Key::Int(1700000050),
        Key::Int(-5),
        Key::Int(1699999999),
        str_key("1700000000.25"),
    ]);
    assert_eq!(
        sorted,
        vec![
            str_key(""),
            Key::Int(-5),
            str_key("0x1A"),
            str_key("10a"),
            Key::Int(3),
            Key::Int(5),
            Key::Int(7),
            Key::Int(42),
            Key::Int(100),
            Key::Int(300),
            str_key("1e9"),
            Key::Int(1699999999),
            Key::Int(1700000000),
            str_key("1700000000.25"),
            Key::Int(1700000001),
            Key::Int(1700000002),
            Key::Int(1700000050),
            str_key("1700000100.5"),
            str_key(" 1700000500"),
            Key::Int(1700001000),
            Key::Int(1700003600),
            Key::Int(1700005000),
            Key::Int(1700007200),
            Key::Int(1700009999),
            str_key("9x"),
            str_key("Z"),
            str_key("a"),
            str_key("abc"),
            str_key("next"),
            str_key("x9"),
        ]
    );
}

#[test]
fn key_sort_places_a_short_mix_as_php_does() {
    // PHP 8.5.12: ksort($a) on these keys (its insertion sort) gives [9,10,"10a",1700000000,"2x",900,"B2","b"].
    let sorted = key_sorted(vec![
        Key::Int(10),
        Key::Int(9),
        str_key("2x"),
        str_key("10a"),
        str_key("b"),
        str_key("B2"),
        Key::Int(1700000000),
        Key::Int(900),
    ]);
    assert_eq!(
        sorted,
        vec![
            Key::Int(9),
            Key::Int(10),
            str_key("10a"),
            Key::Int(1700000000),
            str_key("2x"),
            Key::Int(900),
            str_key("B2"),
            str_key("b"),
        ]
    );
}

#[test]
fn earliest_skips_junk() {
    assert_eq!(CronArray::from_value(sample()).earliest(), Some(100));
    let junk_first = arr(vec![
        (Key::Str("x".into()), s("y")),
        (Key::Int(50), arr(vec![])),
    ]);
    assert_eq!(CronArray::from_value(junk_first).earliest(), Some(50));
    assert_eq!(CronArray::from_value(arr(vec![])).earliest(), None);
}

#[test]
fn event_key_parse_rejects_non_hex() {
    assert!(EventKey::parse(K1).is_some());
    assert!(EventKey::parse("zz").is_none());
    assert!(EventKey::parse(&"g".repeat(32)).is_none());
}
