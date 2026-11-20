mod support;

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_hooks::Key;
use speedyspec_wp_db::{MemoryWpdbHost, WpValue};
use support::{filter, rig, rig_with};

fn saving() -> MemoryWpdbHost {
    let host = MemoryWpdbHost::new();
    host.define("SAVEQUERIES", WpValue::Bool(true));
    host
}

#[test]
fn no_log_without_savequeries() {
    let r = rig();
    r.db.query("SELECT 1").unwrap();
    assert!(r.db.query_log().is_empty());
    assert!(r.db.store().introspect().server_version().is_ok());
}

#[test]
fn logs_with_savequeries() {
    let r = rig_with(saving());
    r.db.query("SELECT 1").unwrap();
    r.db.query("SELECT 2").unwrap();
    let log = r.db.query_log();
    assert_eq!(log.len(), 2);
    assert_eq!(log[0].query, "SELECT 1");
    assert_eq!(log[1].query, "SELECT 2");
    assert_eq!(log[0].engine, "default");
    assert_eq!(log[0].emitted, vec!["SELECT 1".to_string()]);
    assert!(log[0].time >= 0.0);
    assert!(log[0].start > 0.0);
    assert_eq!(log[0].data, WpValue::Array(vec![]));
}

#[test]
fn custom_data_filter() {
    let r = rig_with(saving());
    let seen = Rc::new(RefCell::new(Vec::new()));
    let saw = seen.clone();
    filter(&r.hooks, "log_query_custom_data", "add", move |args| {
        *saw.borrow_mut() = args.to_vec();
        Ok(WpValue::Array(vec![(
            Key::Str("k".into()),
            WpValue::Str("v".into()),
        )]))
    });
    r.db.query("SELECT 1").unwrap();
    assert_eq!(
        r.db.query_log()[0].data,
        WpValue::Array(vec![(Key::Str("k".into()), WpValue::Str("v".into()))])
    );
    let args = seen.borrow();
    assert_eq!(args.len(), 5);
    assert_eq!(args[1], WpValue::Str("SELECT 1".into()));
}
