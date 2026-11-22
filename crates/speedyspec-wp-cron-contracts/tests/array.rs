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

#[test]
fn natural_sort_matches_strnatcasecmp() {
    // PHP 8.5.12: uksort($a, 'strnatcasecmp') on these keys gives
    // ["2x",9,10,"10a",900,1700000000,"b","B2"].
    let keys = vec![
        Key::Int(10),
        Key::Int(9),
        Key::Str("2x".into()),
        Key::Str("10a".into()),
        Key::Str("b".into()),
        Key::Str("B2".into()),
        Key::Int(1700000000),
        Key::Int(900),
    ];
    let mut cron = CronArray::from_value(arr(keys.into_iter().map(|k| (k, arr(vec![]))).collect()));
    cron.natural_sort();
    let sorted: Vec<Key> = match cron.to_value() {
        Value::Array(items) => items.into_iter().map(|(k, _)| k).collect(),
        _ => unreachable!(),
    };
    assert_eq!(
        sorted,
        vec![
            Key::Str("2x".into()),
            Key::Int(9),
            Key::Int(10),
            Key::Str("10a".into()),
            Key::Int(900),
            Key::Int(1700000000),
            Key::Str("b".into()),
            Key::Str("B2".into()),
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
