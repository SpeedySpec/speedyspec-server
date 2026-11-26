//! `NativeWpdb` implements the `Wpdb` port: every method reaches `wpdb` through `&dyn Wpdb`, so none of them calls
//! itself.

mod support;

use speedyspec_data::ExecResult;
use speedyspec_data_query::parse_one;
use speedyspec_wp_db::{RequestState, NativeWpdb, WpdbState};
use speedyspec_wp_db_contracts::{WpDbError, WpValue, Wpdb, take_hook_error};
use speedyspec_wp_hooks_contracts::HookError;
use support::{Answer, Rig, filter, rig, rows};

fn s(v: &str) -> WpValue {
    WpValue::Str(v.into())
}

fn read<T>(state: &dyn WpdbState, f: impl Fn(&RequestState) -> T) -> T {
    let mut out = None;
    state.read(&mut |s| out = Some(f(s)));
    out.unwrap()
}

/// `wp_options`' columns, for the helpers' `process_fields`.
fn options_columns(r: &Rig) {
    let cols = rows(
        &["Field", "Type", "Collation"],
        &[
            &[
                Some("option_name"),
                Some("varchar(191)"),
                Some("utf8mb4_unicode_520_ci"),
            ],
            &[
                Some("option_value"),
                Some("longtext"),
                Some("utf8mb4_unicode_520_ci"),
            ],
        ],
    );
    r.store
        .by_text
        .borrow_mut()
        .push(("SHOW FULL COLUMNS FROM `wp_options`".into(), cols));
}

fn sent(r: &Rig, verb: &str) -> usize {
    r.store
        .texts()
        .iter()
        .filter(|t| t.starts_with(verb))
        .count()
}

#[test]
fn every_method_reaches_wpdb_through_dyn() {
    let r = rig();
    options_columns(&r);
    let db: &dyn Wpdb = &r.db;

    assert_eq!(db.table_name("options"), NativeWpdb::table_name(&r.db, "options"));

    let before = read(&*r.state, |s| s.suppress_errors);
    assert_eq!(db.suppress_errors(true), before);
    assert!(db.suppress_errors(before));

    let data = [
        ("option_name".to_string(), s("a")),
        ("option_value".to_string(), s("b")),
    ];
    r.store.answer(Answer::Exec(ExecResult {
        rows_affected: 1,
        insert_id: Some(42),
    }));
    assert_eq!(db.insert("wp_options", &data, None).unwrap(), 1);
    assert_eq!(sent(&r, "INSERT"), 1);
    assert_eq!(db.insert_id(), 42);

    let conditions = [("option_name".to_string(), s("a"))];
    db.update("wp_options", &data[1..], &conditions, None, None)
        .unwrap();
    assert_eq!(sent(&r, "UPDATE"), 1);
    db.delete("wp_options", &conditions, None).unwrap();
    assert_eq!(sent(&r, "DELETE"), 1);

    let queries = read(&*r.state, |s| s.num_queries);
    db.query_statement(parse_one("SELECT 1").unwrap()).unwrap();
    assert_eq!(read(&*r.state, |s| s.num_queries), queries + 1);
    db.query_bytes(b"SELECT 2").unwrap();
    assert_eq!(read(&*r.state, |s| s.num_queries), queries + 2);

    assert!(db.prepare(Some(b"SELECT %d"), &[WpValue::Int(1)]).is_some());
    assert!(db.process_fields("wp_options", &data, None).is_ok());
    assert_eq!(db.charset(), NativeWpdb::charset(&r.db));
}

#[test]
fn insert_id_matches_the_property() {
    let r = rig();
    options_columns(&r);
    let db: &dyn Wpdb = &r.db;
    assert_eq!(
        Some(WpValue::Int(db.insert_id() as i64)),
        r.db.property("insert_id")
    );

    r.store.answer(Answer::Exec(ExecResult {
        rows_affected: 1,
        insert_id: Some(7),
    }));
    db.insert("wp_options", &[("option_name".to_string(), s("a"))], None)
        .unwrap();
    assert_eq!(db.insert_id(), 7);
    assert_eq!(
        Some(WpValue::Int(db.insert_id() as i64)),
        r.db.property("insert_id")
    );
}

#[test]
fn hook_error_through_wpdb_is_taken_once() {
    let r = rig();
    let _ = take_hook_error();
    filter(&r.hooks, "query", "throws", |_| {
        Err(HookError::message("boom"))
    });
    let db: &dyn Wpdb = &r.db;
    assert!(matches!(
        db.query_statement(parse_one("SELECT 1").unwrap()),
        Err(WpDbError::Hook(_))
    ));
    assert_eq!(
        take_hook_error().map(|e| e.message),
        Some("boom".to_string())
    );
    assert!(take_hook_error().is_none());
}
