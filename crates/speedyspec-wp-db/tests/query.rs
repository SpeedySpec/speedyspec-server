mod support;

use std::rc::Rc;

use speedyspec_data::{DbError, ExecResult, StatementKind};
use speedyspec_data_query::parse_one;
use speedyspec_wp_db::{HostCall, QueryOutcome, WpDbError, WpValue, WpdbState};
use speedyspec_wp_hooks::HookName;
use support::{Answer, filter, rig, rows};

fn read<T>(state: &dyn WpdbState, f: impl Fn(&speedyspec_wp_db::RequestState) -> T) -> T {
    let mut out = None;
    state.read(&mut |s| out = Some(f(s)));
    out.unwrap()
}

#[test]
fn select_sets_state() {
    let r = rig();
    r.store.answer(Answer::Rows(rows(&["a"], &[&[Some("1")]])));
    match r.db.query("SELECT a FROM t").unwrap() {
        QueryOutcome::Rows(rows) => assert_eq!(rows.len(), 1),
        other => panic!("{other:?}"),
    }
    assert_eq!(read(&*r.state, |s| s.num_rows), 1);
    assert_eq!(
        read(&*r.state, |s| s.last_query.as_ref().unwrap().text()),
        b"SELECT a FROM t".to_vec()
    );
    assert_eq!(read(&*r.state, |s| s.num_queries), 1);
    assert_eq!(read(&*r.state, |s| s.result_generation), 1);
    assert_eq!(r.hooks.did_filter(&HookName::from("query")).unwrap(), 1);
    assert_eq!(QueryOutcome::Rows(Rc::new(vec![])).to_wp(), WpValue::Int(0));
}

#[test]
fn insert_sets_rows_and_id() {
    let r = rig();
    r.store.answer(Answer::Exec(ExecResult {
        rows_affected: 1,
        insert_id: Some(7),
    }));
    assert_eq!(
        r.db.query("INSERT INTO t VALUES (1)").unwrap(),
        QueryOutcome::Affected {
            rows: 1,
            insert_id: 7
        }
    );
    assert_eq!(read(&*r.state, |s| (s.rows_affected, s.insert_id)), (1, 7));
}

#[test]
fn comment_first_insert_is_read() {
    let r = rig();
    r.state.modify(&mut |s| {
        s.rows_affected = 4;
        s.insert_id = 9;
    });
    r.store.answer(Answer::Exec(ExecResult {
        rows_affected: 1,
        insert_id: Some(7),
    }));
    match r.db.query("/* t */ INSERT INTO t VALUES (1)").unwrap() {
        QueryOutcome::Rows(rows) => assert!(rows.is_empty()),
        other => panic!("{other:?}"),
    }
    // flush() clears rows_affected as WordPress's does; insert_id is untouched.
    assert_eq!(read(&*r.state, |s| (s.rows_affected, s.insert_id)), (0, 9));
    assert_eq!(r.store.seen.borrow()[0].1, StatementKind::Insert);
}

#[test]
fn ddl_returns_done() {
    let r = rig();
    assert_eq!(
        r.db.query("CREATE TABLE t (a int)").unwrap(),
        QueryOutcome::Done
    );
    assert_eq!(QueryOutcome::Done.to_wp(), WpValue::Bool(true));
}

#[test]
fn query_filter_rewrites() {
    let r = rig();
    filter(&r.hooks, "query", "rewrite", |_| {
        Ok(WpValue::Str("SELECT 2".into()))
    });
    r.db.query("SELECT 1").unwrap();
    assert_eq!(r.store.texts(), vec!["SELECT 2"]);
    assert_eq!(
        read(&*r.state, |s| s.last_query.as_ref().unwrap().text()),
        b"SELECT 2".to_vec()
    );
}

#[test]
fn query_filter_empties() {
    let r = rig();
    r.state.modify(&mut |s| s.insert_id = 5);
    filter(&r.hooks, "query", "empty", |_| {
        Ok(WpValue::Str(String::new()))
    });
    assert!(matches!(r.db.query("SELECT 1"), Err(WpDbError::EmptyQuery)));
    assert!(r.store.texts().is_empty());
    assert_eq!(read(&*r.state, |s| s.insert_id), 0);
}

#[test]
fn placeholder_removed_before_execution() {
    let r = rig();
    let p = r.db.placeholder_escape().placeholder().to_string();
    r.db.query(&format!("SELECT '50{p}'")).unwrap();
    assert_eq!(r.store.texts(), vec!["SELECT '50%'"]);
    assert_eq!(
        read(&*r.state, |s| s.last_query.as_ref().unwrap().text()),
        b"SELECT '50%'".to_vec()
    );

    let r = rig();
    let p = r.db.placeholder_escape().placeholder().to_string();
    let seen = Rc::new(std::cell::RefCell::new(String::new()));
    let saw = seen.clone();
    filter(&r.hooks, "query", "watch", move |args| {
        if let WpValue::Str(s) = &args[0] {
            *saw.borrow_mut() = s.clone();
        }
        Ok(args[0].clone())
    });
    r.db.query(&format!("SELECT '50{p}'")).unwrap();
    assert_eq!(r.store.texts(), vec!["SELECT '50%'"]);
    // WordPress removes the escape in a priority-0 callback, before the others run.
    assert_eq!(*seen.borrow(), "SELECT '50%'");
    let _ = p;
}

#[test]
fn parse_error_is_last_error() {
    let r = rig();
    assert!(matches!(
        r.db.query("SELEC 1"),
        Err(WpDbError::Db(DbError::Parse { .. }))
    ));
    assert!(read(&*r.state, |s| s.last_error.clone()).starts_with("Could not parse the query:"));
    let records = r
        .host
        .calls()
        .into_iter()
        .filter(|c| matches!(c, HostCall::RecordError(..)))
        .count();
    assert_eq!(records, 1);
    assert!(r.store.texts().is_empty());
}

#[test]
fn engine_error_prints() {
    let r = rig();
    r.db.show_errors(true);
    r.store.answer(Answer::Err(DbError::Engine {
        engine: "default".into(),
        code: Some("1146".into()),
        message: "Table 'x.t' doesn't exist".into(),
    }));
    assert!(r.db.query("SELECT * FROM t").is_err());
    assert_eq!(
        read(&*r.state, |s| s.last_error.clone()),
        "Table 'x.t' doesn't exist"
    );
    let output = r.host.calls().into_iter().find_map(|c| match c {
        HostCall::Output(html) => Some(html),
        _ => None,
    });
    assert!(output.unwrap().contains(
        "<strong>WordPress database error:</strong> [Table &#039;x.t&#039; doesn&#039;t exist]"
    ));
    assert!(r.host.calls().iter().any(|c| matches!(c, HostCall::ErrorLog(m, None) if m.starts_with("WordPress database error Table 'x.t' doesn't exist for query SELECT * FROM t"))));
}

#[test]
fn suppressed_errors_only_record() {
    let r = rig();
    r.db.show_errors(true);
    r.db.suppress_errors(true);
    r.store.answer(Answer::Err(DbError::Engine {
        engine: "default".into(),
        code: None,
        message: "nope".into(),
    }));
    assert!(r.db.query("SELECT 1").is_err());
    assert_eq!(
        r.host.calls(),
        vec![HostCall::RecordError("SELECT 1".into(), "nope".into())]
    );
}

#[test]
fn not_ready_refuses() {
    let r = rig();
    r.state.modify(&mut |s| {
        s.ready = false;
        s.check_current_query = false;
    });
    assert!(matches!(r.db.query("SELECT 1"), Err(WpDbError::NotReady)));
    assert!(read(&*r.state, |s| s.check_current_query));
}

#[test]
fn query_statement_fast_path() {
    let r = rig();
    let stmt = parse_one("SELECT option_value FROM wp_options WHERE option_name = 'a'").unwrap();
    r.db.query_statement(stmt).unwrap();
    assert_eq!(
        r.store.texts(),
        vec!["SELECT option_value FROM wp_options WHERE option_name = 'a'"]
    );
    assert_eq!(read(&*r.state, |s| s.num_queries), 1);
    assert_eq!(r.hooks.did_filter(&HookName::from("query")).unwrap(), 1);
    assert!(matches!(
        read(&*r.state, |s| s.last_query.clone()),
        Some(speedyspec_wp_db::LastQuery::Statement(_))
    ));
    assert_eq!(
        read(&*r.state, |s| s.last_query.as_ref().unwrap().text()),
        b"SELECT option_value FROM wp_options WHERE option_name = 'a'".to_vec()
    );
}

#[test]
fn query_statement_reparses_with_callback() {
    let r = rig();
    filter(&r.hooks, "query", "limit", |args| match &args[0] {
        WpValue::Str(s) => Ok(WpValue::Str(format!("{s} LIMIT 1"))),
        other => Ok(other.clone()),
    });
    r.db.query_statement(parse_one("SELECT a FROM t").unwrap())
        .unwrap();
    assert_eq!(r.store.texts(), vec!["SELECT a FROM t LIMIT 1"]);
}

#[test]
fn statement_hook_replaces_tree() {
    let r = rig();
    filter(&r.hooks, "speedyspec_query_statement", "swap", |_| {
        Ok(WpValue::Opaque(Rc::new(parse_one("SELECT 42").unwrap())))
    });
    r.db.query("SELECT 1").unwrap();
    assert_eq!(r.store.texts(), vec!["SELECT 42"]);
}

#[test]
fn connection_error_retries_once() {
    let r = rig();
    r.store.answer(Answer::Err(DbError::Connection {
        engine: "default".into(),
        message: "gone".into(),
    }));
    r.store.answer(Answer::Rows(rows(&["a"], &[&[Some("1")]])));
    assert!(
        matches!(r.db.query("SELECT a FROM t").unwrap(), QueryOutcome::Rows(rows) if rows.len() == 1)
    );
    assert_eq!(read(&*r.state, |s| s.num_queries), 2);
}

#[test]
fn binary_text_reaches_store_as_bytes() {
    let r = rig();
    // A latin1 table holds any byte, so the query-level charset check lets it through.
    r.store.by_text.borrow_mut().push((
        "SHOW FULL COLUMNS FROM `t`".into(),
        rows(
            &["Field", "Type", "Collation"],
            &[&[Some("a"), Some("varchar(20)"), Some("latin1_swedish_ci")]],
        ),
    ));
    let out =
        r.db.query_bytes(b"INSERT INTO t (a) VALUES ('\xE9\x00\xFF')");
    assert!(out.is_ok(), "{out:?} {:?}", r.store.texts());
    let seen = r.store.seen.borrow();
    let insert = seen
        .iter()
        .find(|(t, _, _)| t.starts_with("INSERT"))
        .unwrap();
    assert_eq!(insert.0, "INSERT INTO t (a) VALUES (?)");
    assert_eq!(
        insert.2,
        vec![speedyspec_data::DbValue::Bytes(vec![0xE9, 0x00, 0xFF])]
    );
}
