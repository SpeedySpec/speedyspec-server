use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Mutex;

use speedyspec_data::EngineConfig;
use speedyspec_data_mysql::MySqlEngineFactory;
use speedyspec_data_query::EngineRegistry;
use speedyspec_wp_db::{MemoryWpdbHost, MemoryWpdbState, WpDb, WpDbConfig, WpDbParts, WpValue};
use speedyspec_wp_hooks::{Callback, HookApi, HookName, HookValue, IgnoreDeprecations};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_object_cache::{CurrentObjectCache, ObjectCachePorts};
use speedyspec_wp_options::Option as OptionEntity;
use speedyspec_wp_options::conformance::option_repository_suite;
use speedyspec_wp_options::{
    Autoload, OptionName, OptionRepository, OptionsApi, OptionsDeps, SerializedValue,
};
use speedyspec_wp_options_sql::SqlOptionRepository;

/// The tests share one `wp_options`: one at a time.
static TABLE: Mutex<()> = Mutex::new(());

const SCHEMA: &str = "CREATE TABLE wp_options (option_id bigint(20) unsigned NOT NULL auto_increment, \
    option_name varchar(191) NOT NULL default '', option_value longtext NOT NULL, \
    autoload varchar(20) NOT NULL default 'yes', PRIMARY KEY (option_id), UNIQUE KEY option_name (option_name), \
    KEY autoload (autoload)) DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci";

struct Rig {
    db: Rc<WpDb>,
    hooks: Rc<HookApi>,
    host: Rc<MemoryWpdbHost>,
    repo: Rc<SqlOptionRepository>,
}

fn rig() -> Rig {
    let url = std::env::var("SPEEDYSPEC_TEST_MYSQL_URL").expect("set SPEEDYSPEC_TEST_MYSQL_URL");
    let registry = EngineRegistry::new();
    registry.register_factory(Rc::new(MySqlEngineFactory));
    registry
        .configure(EngineConfig {
            name: "default".into(),
            url,
            options: BTreeMap::new(),
        })
        .unwrap();
    let hooks = Rc::new(HookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let host = Rc::new(MemoryWpdbHost::new());
    let db = Rc::new(WpDb::new(WpDbParts {
        store: registry.sql("default").unwrap(),
        hooks: hooks.clone(),
        state: Rc::new(MemoryWpdbState::new()),
        host: host.clone(),
        config: WpDbConfig {
            user: "root".into(),
            password: "password".into(),
            name: "speedyspec_test".into(),
            host: "127.0.0.1:33306".into(),
            charset: Some("utf8mb4".into()),
            collate: None,
            table_prefix: "wp_".into(),
        },
    }));
    db.db_connect(true).unwrap();
    // As wp-settings.php does, so `$wpdb->options` is `wp_options`.
    db.set_prefix("wp_", true).unwrap();
    db.query("DROP TABLE IF EXISTS wp_options").unwrap();
    db.query(SCHEMA).unwrap();
    let repo = Rc::new(SqlOptionRepository::new(db.clone(), "default".into()));
    Rig {
        db,
        hooks,
        host,
        repo,
    }
}

fn num_queries(db: &WpDb) -> i64 {
    match db.property("num_queries") {
        Some(WpValue::Int(n)) => n,
        other => panic!("{other:?}"),
    }
}

fn on_query(hooks: &HookApi, run: impl Fn(&HookValue) -> HookValue + 'static) {
    hooks
        .add_filter(
            &HookName::from("query"),
            Callback::rust("test_query", move |args: &[HookValue]| Ok(run(&args[0]))),
            &HookValue::Int(10),
            1,
        )
        .unwrap();
}

fn name(n: &str) -> OptionName {
    OptionName::new(n).unwrap()
}

fn option(n: &str, value: &[u8], autoload: Autoload) -> OptionEntity {
    OptionEntity {
        name: name(n),
        value: SerializedValue::new(value.to_vec()),
        autoload,
    }
}

#[test]
#[ignore = "needs MySQL: set SPEEDYSPEC_TEST_MYSQL_URL"]
fn mysql_conformance() {
    let _one = TABLE.lock().unwrap_or_else(|e| e.into_inner());
    let r = rig();
    option_repository_suite(&*r.repo);
    assert_eq!(r.repo.engine(), "default");
}

#[test]
#[ignore = "needs MySQL: set SPEEDYSPEC_TEST_MYSQL_URL"]
fn query_fires_and_counts() {
    let _one = TABLE.lock().unwrap_or_else(|e| e.into_inner());
    let r = rig();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let saw = seen.clone();
    on_query(&r.hooks, move |q| {
        saw.borrow_mut().push(q.clone());
        q.clone()
    });
    let before = num_queries(&r.db);
    r.repo.add(&option("a", b"1", Autoload::On)).unwrap();
    r.repo.value(&name("a"), false).unwrap();
    r.repo.autoloaded(&["on".into()]).unwrap();
    assert_eq!(num_queries(&r.db) - before, 3);
    let texts: Vec<String> = seen
        .borrow()
        .iter()
        .map(|q| match q {
            HookValue::Str(s) => s.clone(),
            other => format!("{other:?}"),
        })
        .collect();
    assert!(
        texts[1].starts_with("SELECT option_value FROM wp_options"),
        "{texts:?}"
    );
    assert_eq!(texts.len(), 3);
}

#[test]
#[ignore = "needs MySQL: set SPEEDYSPEC_TEST_MYSQL_URL"]
fn emptied_update_fails() {
    let _one = TABLE.lock().unwrap_or_else(|e| e.into_inner());
    let r = rig();
    r.repo.add(&option("a", b"1", Autoload::On)).unwrap();
    r.repo.add(&option("b", b"2", Autoload::On)).unwrap();
    let current = Rc::new(CurrentObjectCache::new(ObjectCachePorts::rust(), None));
    let cache = current.current();
    let api = OptionsApi::new(OptionsDeps::rust(r.hooks.clone(), current, r.repo.clone()));
    let before = api.load_alloptions(false).unwrap();
    on_query(&r.hooks, |q| match q {
        HookValue::Str(s) if s.starts_with("UPDATE ") => HookValue::Str(String::new()),
        other => other.clone(),
    });
    assert_eq!(
        r.repo.set_autoload(&["a".into()], &Autoload::Off).unwrap(),
        0
    );
    let results = api
        .set_option_autoload_values(vec![
            ("a".into(), HookValue::Bool(false)),
            ("b".into(), HookValue::Bool(false)),
        ])
        .unwrap();
    assert_eq!(results, vec![("a".into(), false), ("b".into(), false)]);
    assert_eq!(
        cache
            .get(
                &HookValue::Str("alloptions".into()),
                &HookValue::Str("options".into()),
                false
            )
            .unwrap(),
        Some(before)
    );
}

#[test]
#[ignore = "needs MySQL: set SPEEDYSPEC_TEST_MYSQL_URL"]
fn invalid_utf8_update_is_zero() {
    let _one = TABLE.lock().unwrap_or_else(|e| e.into_inner());
    let r = rig();
    r.repo.add(&option("a", b"ok", Autoload::On)).unwrap();
    let bad = SerializedValue::new(vec![0x61, 0xff]);
    assert_eq!(r.repo.update(&name("a"), Some(&bad), None).unwrap(), 0);
    assert_eq!(
        r.repo.value(&name("a"), false).unwrap(),
        Some(SerializedValue::new(b"ok".to_vec()))
    );
    // wpdb::query refuses add_option's INSERT too: WordPress 7.1.5's add_option of these bytes returns false.
    assert_eq!(
        r.repo
            .add(&option("b", &[0x61, 0xff], Autoload::On))
            .unwrap(),
        0
    );
    assert_eq!(r.repo.value(&name("b"), false).unwrap(), None);
}

#[test]
#[ignore = "needs MySQL: set SPEEDYSPEC_TEST_MYSQL_URL"]
fn per_site_table() {
    let _one = TABLE.lock().unwrap_or_else(|e| e.into_inner());
    let r = rig();
    r.host.define("MULTISITE", WpValue::Bool(true));
    *r.host.multisite.borrow_mut() = true;
    let seen = Rc::new(RefCell::new(Vec::new()));
    let saw = seen.clone();
    on_query(&r.hooks, move |q| {
        saw.borrow_mut().push(q.clone());
        q.clone()
    });
    r.db.set_blog_id(2, 1);
    let _ = r.repo.value(&name("a"), true);
    let last = seen.borrow().last().cloned();
    assert!(
        matches!(&last, Some(HookValue::Str(s)) if s.contains("wp_2_options")),
        "{last:?}"
    );
}
