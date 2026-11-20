use speedyspec_hooks::{Key, Value};
use speedyspec_wp_db::{PlaceholderEscape, Prepare, esc_like, escape, real_escape};

fn s(v: &str) -> Value {
    Value::Str(v.into())
}

fn prepare(
    escape: &PlaceholderEscape,
    query: &str,
    args: &[Value],
) -> (Option<String>, Vec<(String, String)>) {
    let out = Prepare {
        escape,
        allow_unsafe_unquoted_parameters: true,
    }
    .prepare(Some(query.as_bytes()), args);
    (
        out.sql.map(|b| String::from_utf8(b).unwrap()),
        out.notices
            .into_iter()
            .map(|n| (n.function, n.version))
            .collect(),
    )
}

#[test]
fn prepare_basic() {
    let e = PlaceholderEscape::new(Some("salt"));
    let (sql, notices) = prepare(
        &e,
        "SELECT * FROM t WHERE a = %s AND b = %d AND c = %f",
        &[s("a'b"), s("12abc"), Value::Float(1.5)],
    );
    assert_eq!(
        sql.as_deref(),
        Some("SELECT * FROM t WHERE a = 'a\\'b' AND b = 12 AND c = 1.500000")
    );
    assert!(notices.is_empty(), "{notices:?}");
}

#[test]
fn prepare_padded_unquoted() {
    let e = PlaceholderEscape::new(Some("salt"));
    for (query, arg, expected) in [
        ("%5s", "foo", "  foo"),
        ("%-5s", "foo", "foo  "),
        ("%05s", "foo", "00foo"),
        ("%'#5s", "foo", "##foo"),
        ("%.3s", "foobar", "foo"),
    ] {
        assert_eq!(
            prepare(&e, query, &[s(arg)]).0.as_deref(),
            Some(expected),
            "{query}"
        );
    }
}

#[test]
fn prepare_identifier() {
    let e = PlaceholderEscape::new(Some("salt"));
    assert_eq!(
        prepare(&e, "SELECT * FROM %i", &[s("my`table")])
            .0
            .as_deref(),
        Some("SELECT * FROM `my``table`")
    );
    let (sql, notices) = prepare(&e, "SELECT %1$i, %1$s", &[s("a")]);
    assert_eq!(sql, None);
    assert_eq!(
        notices,
        vec![("wpdb::prepare".to_string(), "6.2.0".to_string())]
    );
}

#[test]
fn prepare_percent_is_escaped() {
    let e = PlaceholderEscape::new(Some("salt"));
    let p = e.placeholder();
    let (sql, notices) = prepare(&e, "%1$d %%% % %%1$d%% %%%1$d%%", &[Value::Int(1)]);
    assert_eq!(sql.unwrap(), format!("1 {p}{p} {p} {p}1$d{p} {p}1{p}"));
    assert_eq!(notices.len(), 1);
}

#[test]
fn prepare_incorrect_arg_count_cases() {
    let e = PlaceholderEscape::new(Some("salt"));
    let p = e.placeholder();
    let u = "SELECT * FROM wp_users WHERE";
    let list = |items: Vec<Value>| {
        Value::Array(
            items
                .into_iter()
                .enumerate()
                .map(|(i, v)| (Key::Int(i as i64), v))
                .collect(),
        )
    };
    let cases: Vec<(String, Vec<Value>, String)> = vec![
        (
            format!("{u} id = %d AND user_login = %s"),
            vec![Value::Int(1), s("admin"), s("extra-arg")],
            format!("{u} id = 1 AND user_login = 'admin'"),
        ),
        (
            format!("{u} id = %%%d AND user_login = %s"),
            vec![Value::Int(1)],
            String::new(),
        ),
        (
            format!("{u} id = %d AND user_login = %s"),
            vec![list(vec![Value::Int(1), s("admin"), s("extra-arg")])],
            format!("{u} id = 1 AND user_login = 'admin'"),
        ),
        (
            format!("{u} id = %d AND %% AND user_login = %s"),
            vec![Value::Int(1), s("admin"), s("extra-arg")],
            format!("{u} id = 1 AND {p} AND user_login = 'admin'"),
        ),
        (
            format!("{u} id = %%%d AND %F AND %f AND user_login = %s"),
            vec![
                Value::Int(1),
                Value::Float(2.3),
                s("4.5"),
                s("admin"),
                s("extra-arg"),
            ],
            format!("{u} id = {p}1 AND 2.300000 AND 4.500000 AND user_login = 'admin'"),
        ),
        (
            format!("{u} id = %d AND user_login = %s"),
            vec![list(vec![Value::Int(1)]), s("admin"), s("extra-arg")],
            format!("{u} id = 0 AND user_login = 'admin'"),
        ),
        (
            format!("{u} id = %d and user_nicename = %s and user_status = %d and user_login = %s"),
            vec![Value::Int(1), s("admin"), Value::Int(0)],
            String::new(),
        ),
        (
            format!("{u} id = %d and user_nicename = %s and user_status = %d and user_login = %s"),
            vec![list(vec![Value::Int(1), s("admin"), Value::Int(0)])],
            String::new(),
        ),
        (
            format!(
                "{u} id = %d and %% and user_login = %s and user_status = %d and user_login = %s"
            ),
            vec![Value::Int(1), s("admin"), s("extra-arg")],
            String::new(),
        ),
    ];
    for (query, args, expected) in cases {
        let (sql, notices) = prepare(&e, &query, &args);
        assert_eq!(sql.as_deref(), Some(expected.as_str()), "{query}");
        assert!(!notices.is_empty(), "{query}: expected a notice");
    }
}

#[test]
fn prepare_wrong_count() {
    let e = PlaceholderEscape::new(Some("salt"));
    let (sql, notices) = prepare(&e, "SELECT %s, %s", &[s("a")]);
    assert_eq!(sql.as_deref(), Some(""));
    assert_eq!(
        notices,
        vec![("wpdb::prepare".to_string(), "4.8.3".to_string())]
    );
}

#[test]
fn prepare_null_query() {
    let e = PlaceholderEscape::new(Some("salt"));
    let out = Prepare {
        escape: &e,
        allow_unsafe_unquoted_parameters: true,
    }
    .prepare(None, &[]);
    assert_eq!(out.sql, None);
    assert!(out.notices.is_empty());
}

#[test]
fn prepare_no_placeholder() {
    let e = PlaceholderEscape::new(Some("salt"));
    let (sql, notices) = prepare(&e, "SELECT 1", &[]);
    assert_eq!(sql.as_deref(), Some("SELECT 1"));
    assert_eq!(
        notices,
        vec![("wpdb::prepare".to_string(), "3.9.0".to_string())]
    );
}

#[test]
fn escape_rules() {
    assert_eq!(real_escape(b"a'b\\c\n\x1a"), b"a\\'b\\\\c\\n\\Z".to_vec());
    assert_eq!(esc_like(b"50%_\\"), b"50\\%\\_\\\\".to_vec());
    let e = PlaceholderEscape::new(Some("salt"));
    let escaped = escape(&Value::Array(vec![(Key::Str("k".into()), s("x%y"))]), &e);
    assert_eq!(
        escaped,
        Value::Array(vec![(
            Key::Str("k".into()),
            Value::Str(format!("x{}y", e.placeholder()))
        )])
    );
    assert_eq!(escape(&Value::Null, &e), s(""));
    assert_eq!(escape(&Value::Int(5), &e), s("5"));
}

#[test]
fn placeholder_round_trip() {
    let e = PlaceholderEscape::new(None);
    assert_eq!(e.remove(&e.add("a%b")), "a%b");
    let p = e.placeholder();
    assert_eq!(p.len(), 66);
    assert!(p.starts_with('{') && p.ends_with('}'));
    assert!(
        p[1..65]
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    );
    assert_ne!(
        PlaceholderEscape::new(Some("salt")).placeholder(),
        PlaceholderEscape::new(Some("salt")).placeholder()
    );
}

#[test]
fn binary_argument_survives() {
    let e = PlaceholderEscape::new(Some("salt"));
    let out = Prepare {
        escape: &e,
        allow_unsafe_unquoted_parameters: true,
    }
    .prepare(Some(b"%s"), &[Value::Bytes(vec![0xE9])]);
    assert_eq!(out.sql, Some(b"'\xE9'".to_vec()));
}
