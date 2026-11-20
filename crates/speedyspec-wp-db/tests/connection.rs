mod support;

use speedyspec_data::DbError;
use speedyspec_wp_db::{HostCall, WpValue, WpdbState};
use speedyspec_wp_hooks::HookName;
use support::{Answer, filter, rig, rows};

#[test]
fn incompatible_modes_filtered() {
    let r = rig();
    r.store.answer(Answer::Rows(rows(
        &["@@SESSION.sql_mode"],
        &[&[Some(
            "STRICT_TRANS_TABLES,NO_ENGINE_SUBSTITUTION,ONLY_FULL_GROUP_BY",
        )]],
    )));
    r.db.set_sql_mode(&[]).unwrap();
    assert_eq!(
        r.store.texts(),
        vec![
            "SELECT @@SESSION.sql_mode",
            "SET SESSION sql_mode = 'NO_ENGINE_SUBSTITUTION'"
        ]
    );
}

#[test]
fn incompatible_modes_filter_hook() {
    let r = rig();
    filter(
        &r.hooks,
        "incompatible_sql_modes",
        "keep",
        |args| match &args[0] {
            WpValue::Array(items) => Ok(WpValue::Array(
                items
                    .iter()
                    .filter(|(_, v)| *v != WpValue::Str("ONLY_FULL_GROUP_BY".into()))
                    .cloned()
                    .collect(),
            )),
            other => Ok(other.clone()),
        },
    );
    r.db.set_sql_mode(&["ONLY_FULL_GROUP_BY".into(), "STRICT_ALL_TABLES".into()])
        .unwrap();
    assert_eq!(
        r.store.texts(),
        vec!["SET SESSION sql_mode = 'ONLY_FULL_GROUP_BY'"]
    );
}

#[test]
fn set_charset_is_not_counted() {
    let r = rig();
    r.db.set_charset(Some("latin1"), Some("latin1_swedish_ci"))
        .unwrap();
    assert_eq!(
        r.store.texts(),
        vec!["SET NAMES 'latin1' COLLATE latin1_swedish_ci"]
    );
    let mut n = 99;
    r.state.read(&mut |s| n = s.num_queries);
    assert_eq!(n, 0);
    assert_eq!(r.hooks.did_filter(&HookName::from("query")).unwrap(), 0);
    // The engine's own charset needs no SET NAMES.
    r.db.set_charset(Some("utf8mb4"), None).unwrap();
    assert_eq!(r.store.texts().len(), 1);
}

#[test]
fn charset_collate_text() {
    let r = rig();
    r.db.init_charset();
    assert_eq!(r.db.charset(), "utf8mb4");
    assert_eq!(r.db.collate(), "utf8mb4_unicode_520_ci");
    assert_eq!(
        r.db.get_charset_collate(),
        "DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_520_ci"
    );
    r.db.set_collate(String::new());
    assert_eq!(r.db.get_charset_collate(), "DEFAULT CHARACTER SET utf8mb4");
    assert!(r.db.supports_collation());
    assert!(r.host.calls().iter().any(|c| matches!(c, HostCall::DeprecatedFunction(f, v, _) if f == "supports_collation" && v == "3.5.0")));
    assert!(r.db.check_database_version("5.5.5", "7.1.5").is_ok());
    assert!(r.db.check_database_version("9.9", "7.1.5").is_err());
}

#[test]
fn close_then_query_not_ready() {
    let r = rig();
    assert!(r.db.close());
    assert!(!r.db.close());
    assert!(r.db.query("SELECT 1").is_err());
}

#[test]
fn check_connection_bails_after_retries() {
    let r = rig();
    r.db.show_errors(true);
    r.db.set_reconnect(1, std::time::Duration::ZERO);
    for _ in 0..4 {
        r.store.answer(Answer::Err(DbError::Connection {
            engine: "default".into(),
            message: "gone".into(),
        }));
    }
    assert!(!r.db.check_connection(true).unwrap());
    let calls = r.host.calls();
    assert!(
        calls.iter().any(
            |c| matches!(c, HostCall::Die(m) if m.contains("Error reconnecting to the database"))
        ),
        "{calls:?}"
    );
    assert!(calls.contains(&HostCall::DeadDb));
}

#[test]
fn db_connect_sets_up_the_session() {
    let r = rig();
    r.store.by_text.borrow_mut().push((
        "SELECT @@SESSION.sql_mode".into(),
        rows(&["@@SESSION.sql_mode"], &[&[Some("NO_ZERO_DATE")]]),
    ));
    assert!(r.db.db_connect(true).unwrap());
    let texts = r.store.texts();
    assert!(
        texts.contains(&"SET NAMES 'utf8mb4' COLLATE utf8mb4_unicode_520_ci".to_string()),
        "{texts:?}"
    );
    assert!(
        texts.contains(&"SET SESSION sql_mode = ''".to_string()),
        "{texts:?}"
    );
    assert!(texts.contains(&"USE `wp`".to_string()), "{texts:?}");
    assert_eq!(r.db.db_version().unwrap(), "8.4.3");
    assert!(r.db.has_cap("utf8mb4_520"));
}
