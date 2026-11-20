mod support;

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_wp_db::{HostCall, OutputShape, Shaped, WpDbError, WpValue, WpdbState};
use support::{Rig, filter, rig, rows};

fn s(v: &str) -> WpValue {
    WpValue::Str(v.into())
}

fn options_columns(r: &Rig) {
    let cols = rows(
        &[
            "Field",
            "Type",
            "Collation",
            "Null",
            "Key",
            "Default",
            "Extra",
            "Privileges",
            "Comment",
        ],
        &[
            &[
                Some("option_id"),
                Some("bigint(20) unsigned"),
                None,
                Some("NO"),
                Some("PRI"),
                None,
                Some("auto_increment"),
                Some(""),
                Some(""),
            ],
            &[
                Some("option_name"),
                Some("varchar(191)"),
                Some("utf8mb4_unicode_520_ci"),
                Some("NO"),
                Some("UNI"),
                Some(""),
                Some(""),
                Some(""),
                Some(""),
            ],
            &[
                Some("option_value"),
                Some("longtext"),
                Some("utf8mb4_unicode_520_ci"),
                Some("NO"),
                Some(""),
                None,
                Some(""),
                Some(""),
                Some(""),
            ],
            &[
                Some("autoload"),
                Some("varchar(20)"),
                Some("utf8mb4_unicode_520_ci"),
                Some("NO"),
                Some("MUL"),
                Some("auto"),
                Some(""),
                Some(""),
                Some(""),
            ],
        ],
    );
    r.store
        .by_text
        .borrow_mut()
        .push(("SHOW FULL COLUMNS FROM `wp_options`".into(), cols));
    let latin = rows(
        &["Field", "Type", "Collation"],
        &[&[Some("name"), Some("varchar(20)"), Some("latin1_swedish_ci")]],
    );
    r.store
        .by_text
        .borrow_mut()
        .push(("SHOW FULL COLUMNS FROM `wp_latin1`".into(), latin));
    let utf8 = rows(
        &["Field", "Type", "Collation"],
        &[&[
            Some("name"),
            Some("varchar(20)"),
            Some("utf8mb3_general_ci"),
        ]],
    );
    r.store
        .by_text
        .borrow_mut()
        .push(("SHOW FULL COLUMNS FROM `wp_utf8`".into(), utf8));
}

fn watch_query(r: &Rig) -> Rc<RefCell<Vec<String>>> {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let saw = seen.clone();
    filter(&r.hooks, "query", "watch", move |args| {
        if let WpValue::Str(q) = &args[0] {
            saw.borrow_mut().push(q.clone());
        }
        Ok(args[0].clone())
    });
    seen
}

#[test]
fn insert_builds_wordpress_text() {
    let r = rig();
    options_columns(&r);
    let seen = watch_query(&r);
    r.store.answer(
        speedyspec_data::ExecResult {
            rows_affected: 1,
            insert_id: Some(3),
        }
        .into_answer(),
    );
    let n =
        r.db.insert(
            "wp_options",
            &[
                ("option_name".into(), s("a")),
                ("option_value".into(), s("b")),
            ],
            None,
        )
        .unwrap();
    assert_eq!(n, 1);
    assert!(
        seen.borrow().contains(
            &"INSERT INTO `wp_options` (`option_name`, `option_value`) VALUES ('a', 'b')"
                .to_string()
        ),
        "{:?}",
        seen.borrow()
    );
}

trait IntoAnswer {
    fn into_answer(self) -> support::Answer;
}
impl IntoAnswer for speedyspec_data::ExecResult {
    fn into_answer(self) -> support::Answer {
        support::Answer::Exec(self)
    }
}

#[test]
fn update_with_null_condition() {
    let r = rig();
    options_columns(&r);
    let seen = watch_query(&r);
    r.db.update(
        "wp_options",
        &[("option_value".into(), s("b"))],
        &[("option_name".into(), WpValue::Null)],
        None,
        None,
    )
    .unwrap();
    assert!(
        seen.borrow().iter().any(
            |q| q == "UPDATE `wp_options` SET `option_value` = 'b' WHERE `option_name` IS NULL"
        ),
        "{:?}",
        seen.borrow()
    );
    r.db.delete("wp_options", &[("option_name".into(), s("a"))], None)
        .unwrap();
    assert!(
        seen.borrow()
            .iter()
            .any(|q| q == "DELETE FROM `wp_options` WHERE `option_name` = 'a'")
    );
}

#[test]
fn value_too_long_fails() {
    let r = rig();
    options_columns(&r);
    let long = "x".repeat(192);
    match r
        .db
        .insert("wp_options", &[("option_name".into(), s(&long))], None)
    {
        Err(WpDbError::ProcessFields(m)) => {
            assert!(m.contains("option_name"), "{m}");
            let mut last = String::new();
            r.state.read(&mut |st| last = st.last_error.clone());
            assert_eq!(last, m);
        }
        other => panic!("{other:?}"),
    }
    assert!(!r.store.texts().iter().any(|t| t.starts_with("INSERT")));
}

#[test]
fn pre_get_table_charset_wins() {
    let r = rig();
    options_columns(&r);
    filter(&r.hooks, "pre_get_table_charset", "latin", |_| {
        Ok(s("latin1"))
    });
    assert_eq!(
        r.db.get_table_charset("wp_options").unwrap(),
        Some("latin1".to_string())
    );
    assert!(r.store.texts().is_empty());
}

#[test]
fn table_charset_is_cached() {
    let r = rig();
    options_columns(&r);
    assert_eq!(
        r.db.get_table_charset("wp_options").unwrap(),
        Some("utf8mb4".to_string())
    );
    assert_eq!(
        r.db.get_table_charset("WP_OPTIONS").unwrap(),
        Some("utf8mb4".to_string())
    );
    assert_eq!(
        r.store
            .texts()
            .iter()
            .filter(|t| t.starts_with("SHOW FULL COLUMNS"))
            .count(),
        1
    );
    assert_eq!(
        r.db.get_col_charset("wp_options", "option_id").unwrap(),
        None
    );
    assert_eq!(
        r.db.get_col_charset("wp_options", "option_name").unwrap(),
        Some("utf8mb4".to_string())
    );
    assert_eq!(
        r.db.get_col_length("wp_options", "option_name")
            .unwrap()
            .map(|l| l.length),
        Some(191)
    );
}

#[test]
fn readers_follow_last_result() {
    let r = rig();
    r.store.answer(support::Answer::Rows(rows(
        &["k", "v"],
        &[&[Some("a"), Some("1")], &[Some("b"), Some("2")]],
    )));
    r.db.query("SELECT k, v FROM t").unwrap();
    assert_eq!(
        r.db.get_col(1),
        vec![Some(b"1".to_vec()), Some(b"2".to_vec())]
    );
    assert_eq!(r.db.get_var(0, 1), Some(b"b".to_vec()));
    let row = r.db.get_row(Some(OutputShape::ArrayN), 1).unwrap();
    assert_eq!(row.fields[1].1, Some(b"2".to_vec()));
    match r.db.get_results(Some(OutputShape::ObjectK)).unwrap() {
        Shaped::Keyed(k) => assert_eq!(
            k.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>(),
            vec![b"a".to_vec(), b"b".to_vec()]
        ),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        r.db.get_col_info("name", -1),
        WpValue::Array(vec![
            (speedyspec_hooks::Key::Int(0), s("k")),
            (speedyspec_hooks::Key::Int(1), s("v"))
        ])
    );
    assert_eq!(r.db.get_col_info("name", 1), s("v"));
    assert_eq!(r.db.get_col_info("catalog", 0), s("def"));
}

#[test]
fn get_row_bad_shape_prints() {
    let r = rig();
    r.db.show_errors(true);
    r.store
        .answer(support::Answer::Rows(rows(&["k"], &[&[Some("a")]])));
    r.db.query("SELECT k FROM t").unwrap();
    assert_eq!(r.db.get_row(None, 0), None);
    assert!(r.host.calls().iter().any(|c| matches!(c, HostCall::RecordError(_, m) if m.contains("Output type must be one of: OBJECT, ARRAY_A, ARRAY_N"))));
}

#[test]
fn prepare_notices_reach_host() {
    let r = rig();
    assert_eq!(
        r.db.prepare(Some(b"SELECT 1"), &[]),
        Some(b"SELECT 1".to_vec())
    );
    let notices: Vec<_> = r
        .host
        .calls()
        .into_iter()
        .filter_map(|c| match c {
            HostCall::DoingItWrong(n) => Some(n.version),
            _ => None,
        })
        .collect();
    assert_eq!(notices, vec!["3.9.0".to_string()]);
}

#[test]
fn invalid_query_text_refused() {
    let r = rig();
    options_columns(&r);
    // latin1 holds any byte, so WordPress lets this one through.
    assert!(
        r.db.query("INSERT INTO wp_latin1 VALUES ('\u{1F600}')")
            .is_ok()
    );
    match r.db.query("INSERT INTO wp_utf8 VALUES ('\u{1F600}')") {
        Err(WpDbError::InvalidData(m)) => assert_eq!(
            m,
            "WordPress database error: Could not perform query because it contains invalid data."
        ),
        other => panic!("{other:?}"),
    }
    let mut last = String::new();
    r.state.read(&mut |st| last = st.last_error.clone());
    assert!(last.contains("contains invalid data"));
    assert!(
        !r.store
            .texts()
            .iter()
            .any(|t| t.starts_with("INSERT INTO wp_utf8"))
    );
}

#[test]
fn get_var_with_sql_runs_it() {
    let r = rig();
    r.store
        .answer(support::Answer::Rows(rows(&["v"], &[&[Some("7")]])));
    assert_eq!(
        r.db.get_var_sql(Some(b"SELECT 7"), 0, 0).unwrap(),
        Some(b"7".to_vec())
    );
}

#[test]
fn get_col_leaves_get_vars_func_call() {
    let r = rig();
    r.store.answer(support::Answer::Rows(rows(
        &["k"],
        &[&[Some("a")], &[Some("b")]],
    )));
    r.db.get_col_sql(Some(b"SELECT k FROM t"), 0).unwrap();
    let mut call = None;
    r.state.read(&mut |s| call = s.func_call.clone());
    assert_eq!(call.as_deref(), Some("$db->get_var(\"\", 0, 1)"));
}

#[test]
fn reset_request_starts_like_a_new_wpdb() {
    let r = rig();
    r.db.set_prefix("wp_", true).unwrap();
    r.db.set_blog_id(3, 1);
    let mut types = indexmap::IndexMap::new();
    types.insert("id".to_string(), "%d".to_string());
    r.db.set_field_types(types);
    r.db.reset_request();
    assert!(r.db.field_types().is_empty());
    assert_eq!(
        r.db.with_tables(|t| (t.blogid(), t.base_prefix().map(str::to_string))),
        (0, None)
    );
}

#[test]
fn parse_failure_counts_as_a_query() {
    let r = rig();
    let _ = r.db.query("SELEC 1");
    let mut n = 0;
    r.state.read(&mut |s| n = s.num_queries);
    assert_eq!(n, 1);
}

#[test]
fn col_info_property_does_not_panic() {
    let r = rig();
    r.store
        .answer(support::Answer::Rows(rows(&["k"], &[&[Some("a")]])));
    r.db.query("SELECT k FROM t").unwrap();
    assert!(matches!(r.db.property("col_info"), Some(WpValue::Array(_))));
}

#[test]
fn property_writes_read_back() {
    let r = rig();
    assert!(r.db.set_property("dbname", &WpValue::Str("other".into())));
    assert_eq!(r.db.property("dbname"), Some(WpValue::Str("other".into())));
    let row = WpValue::Array(vec![(
        speedyspec_hooks::Key::Str("a".into()),
        WpValue::Str("x".into()),
    )]);
    r.db.set_property(
        "last_result",
        &WpValue::Array(vec![(speedyspec_hooks::Key::Int(0), row)]),
    );
    assert_eq!(r.db.get_var(0, 0), Some(b"x".to_vec()));
}

#[test]
fn object_k_keeps_first_row_index() {
    let r = rig();
    r.store.answer(support::Answer::Rows(rows(
        &["k", "v"],
        &[
            &[Some("a"), Some("1")],
            &[Some("a"), Some("2")],
            &[Some("b"), Some("3")],
        ],
    )));
    r.db.query("SELECT k, v FROM t").unwrap();
    match r.db.get_results(Some(OutputShape::ObjectK)).unwrap() {
        Shaped::Keyed(k) => assert_eq!(
            k.iter()
                .map(|(key, row)| (key.clone(), row.fields[1].1.clone()))
                .collect::<Vec<_>>(),
            vec![
                (b"a".to_vec(), Some(b"1".to_vec())),
                (b"b".to_vec(), Some(b"3".to_vec()))
            ]
        ),
        other => panic!("{other:?}"),
    }
}
