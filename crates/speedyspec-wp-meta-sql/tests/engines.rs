//! `SqlMetaRepository` over `NativeWpdb` on every engine: the conformance suite for `post` and `user`, plus the
//! charset, prefix and missing-table cases. MySQL and PostgreSQL run when `SPEEDYSPEC_TEST_MYSQL_URL` or
//! `SPEEDYSPEC_TEST_POSTGRES_URL` is set, and print `skipped` otherwise.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Mutex;

use speedyspec_data::{EngineConfig, EngineFactory};
use speedyspec_data_mysql::MySqlEngineFactory;
use speedyspec_data_postgres::PostgresEngineFactory;
use speedyspec_data_query::EngineRegistry;
use speedyspec_data_sqlite::SqliteEngineFactory;
use speedyspec_wp_db::{MemoryWpdbHost, MemoryWpdbState, NativeWpdb, WpDbConfig, WpDbParts};
use speedyspec_wp_db_contracts::{WpValue, take_hook_error};
use speedyspec_wp_hooks::{IgnoreDeprecations, WpHookApi};
use speedyspec_wp_hooks_contracts::{Callback, HookError, HookName};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_meta_contracts::conformance::meta_repository_suite;
use speedyspec_wp_meta_contracts::{
    MetaError, MetaId, MetaKey, MetaRepositories, MetaRepository, MetaType, ObjectId,
    SerializedValue,
};
use speedyspec_wp_meta_sql::SqlMetaRepositories;

/// The tests share the meta tables: one at a time.
static TABLES: Mutex<()> = Mutex::new(());

/// The engines the repository must run on.
#[derive(Clone, Copy)]
enum Engine {
    MySql,
    Sqlite,
    Postgres,
}

impl Engine {
    fn name(self) -> &'static str {
        match self {
            Engine::MySql => "mysql",
            Engine::Sqlite => "sqlite",
            Engine::Postgres => "postgres",
        }
    }
}

/// The PostgreSQL schema these tests own, dropped and made afresh for each test.
const PG_SCHEMA: &str = "t_meta_engines";

/// `wp_postmeta` (or any type's table but users'), as `wp-admin/includes/schema.php` creates it.
fn meta_table(table: &str, object_column: &str, id_column: &str) -> String {
    format!(
        "CREATE TABLE {table} ({id_column} bigint(20) unsigned NOT NULL auto_increment, \
         {object_column} bigint(20) unsigned NOT NULL default '0', meta_key varchar(255) default NULL, \
         meta_value longtext, PRIMARY KEY ({id_column}), KEY {object_column} ({object_column}), \
         KEY meta_key (meta_key(191))) DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci"
    )
}

struct Rig {
    db: Rc<NativeWpdb>,
    hooks: Rc<WpHookApi>,
    host: Rc<MemoryWpdbHost>,
    repos: SqlMetaRepositories,
}

/// `NativeWpdb` over `engine` with fresh `wp_postmeta`, `wp_usermeta` and `wp_2_postmeta` tables; `None` when the
/// engine's URL is unset.
fn rig(engine: Engine) -> Option<Rig> {
    let (factory, url, options): (Rc<dyn EngineFactory>, String, BTreeMap<String, String>) =
        match engine {
            Engine::MySql => (
                Rc::new(MySqlEngineFactory),
                std::env::var("SPEEDYSPEC_TEST_MYSQL_URL").ok()?,
                BTreeMap::new(),
            ),
            Engine::Sqlite => (
                Rc::new(SqliteEngineFactory),
                "sqlite://:memory:".into(),
                BTreeMap::from([("database".into(), "speedyspec_test".into())]),
            ),
            Engine::Postgres => {
                let url = std::env::var("SPEEDYSPEC_TEST_POSTGRES_URL").ok()?;
                let mut admin =
                    postgres::Client::connect(&url, postgres::NoTls).expect("PostgreSQL is up");
                admin
                    .batch_execute(&format!(
                        "DROP SCHEMA IF EXISTS {PG_SCHEMA} CASCADE; CREATE SCHEMA {PG_SCHEMA}"
                    ))
                    .unwrap();
                (
                    Rc::new(PostgresEngineFactory),
                    url,
                    BTreeMap::from([("schema".into(), PG_SCHEMA.into())]),
                )
            }
        };
    let database = match engine {
        Engine::MySql => url.rsplit('/').next().unwrap_or_default().to_string(),
        _ => "speedyspec_test".to_string(),
    };
    let registry = EngineRegistry::new();
    registry.register_factory(factory);
    registry
        .configure(EngineConfig {
            name: "default".into(),
            url,
            options,
        })
        .unwrap();
    let hooks = Rc::new(WpHookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let host = Rc::new(MemoryWpdbHost::new());
    let db = Rc::new(NativeWpdb::new(WpDbParts {
        store: registry.sql("default").unwrap(),
        hooks: hooks.clone(),
        state: Rc::new(MemoryWpdbState::new()),
        host: host.clone(),
        config: WpDbConfig {
            user: String::new(),
            password: String::new(),
            name: database,
            host: "localhost".into(),
            charset: Some("utf8mb4".into()),
            collate: None,
            table_prefix: "wp_".into(),
        },
    }));
    db.db_connect(true).unwrap();
    db.set_prefix("wp_", true).unwrap();
    for (table, object_column, id_column) in [
        ("wp_postmeta", "post_id", "meta_id"),
        ("wp_usermeta", "user_id", "umeta_id"),
        ("wp_2_postmeta", "post_id", "meta_id"),
    ] {
        db.query(&format!("DROP TABLE IF EXISTS {table}")).unwrap();
        db.query(&meta_table(table, object_column, id_column))
            .unwrap();
    }
    let repos = SqlMetaRepositories::new(db.clone(), engine.name());
    Some(Rig {
        db,
        hooks,
        host,
        repos,
    })
}

fn of(name: &str) -> MetaType {
    MetaType::new(name).unwrap()
}

fn repo(rig: &Rig, name: &str) -> Rc<dyn MetaRepository> {
    rig.repos.for_type(&of(name)).expect("the type has a table")
}

fn count(db: &NativeWpdb, table: &str) -> u64 {
    db.query(&format!("SELECT COUNT(*) FROM {table}")).unwrap();
    db.get_col(0)
        .into_iter()
        .flatten()
        .next()
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// Runs `test` on `engine`, or prints `skipped` when its URL is unset.
fn on(engine: Engine, test: impl FnOnce(Rig)) {
    let _one = TABLES.lock().unwrap_or_else(|e| e.into_inner());
    match rig(engine) {
        Some(rig) => test(rig),
        None => println!("skipped: no {} URL is set", engine.name()),
    }
}

fn suite(engine: Engine) {
    on(engine, |rig| {
        for name in ["post", "user"] {
            let repo = repo(&rig, name);
            assert_eq!(repo.engine(), engine.name());
            assert_eq!(repo.meta_type(), &of(name));
            meta_repository_suite(&*repo);
        }
    });
}

#[test]
fn suite_on_mysql() {
    suite(Engine::MySql);
}

#[test]
fn suite_on_sqlite() {
    suite(Engine::Sqlite);
}

#[test]
fn suite_on_postgres() {
    suite(Engine::Postgres);
}

/// `wpdb::process_fields()` strips bytes utf8mb4 cannot hold, so `$wpdb->insert` returns false.
#[test]
fn utf8mb4_rejects_invalid_bytes() {
    on(Engine::MySql, |rig| {
        let repo = repo(&rig, "post");
        let added = repo
            .add(
                ObjectId(1),
                &MetaKey::from("k"),
                Some(&SerializedValue::new(vec![0xff])),
            )
            .unwrap();
        assert_eq!(added, None);
        assert_eq!(count(&rig.db, "wp_postmeta"), 0);
    });
}

fn follows_switched_prefix(engine: Engine) {
    on(engine, |rig| {
        let repo = repo(&rig, "post");
        rig.db.set_prefix("wp_2_", true).unwrap();
        let mid = repo
            .add(
                ObjectId(1),
                &MetaKey::from("k"),
                Some(&SerializedValue::new(b"v".to_vec())),
            )
            .unwrap()
            .expect("an id");
        assert_eq!(count(&rig.db, "wp_2_postmeta"), 1);
        assert_eq!(count(&rig.db, "wp_postmeta"), 0);
        assert!(repo.by_mid(mid).unwrap().is_some());
    });
}

#[test]
fn follows_switched_prefix_on_sqlite() {
    follows_switched_prefix(Engine::Sqlite);
}

#[test]
fn follows_switched_prefix_on_mysql() {
    follows_switched_prefix(Engine::MySql);
}

/// `_get_meta_table( 'thing' )` is false until `$wpdb->thingmeta` is set.
#[test]
fn unknown_type_has_no_table() {
    on(Engine::Sqlite, |rig| {
        let thing = of("thing");
        assert!(rig.repos.for_type(&thing).is_none());
        rig.db
            .query(&meta_table("wp_thingmeta", "thing_id", "meta_id"))
            .unwrap();
        assert!(rig.repos.for_type(&thing).is_none());
        rig.db
            .with_tables_mut(|t| t.set("thingmeta", "wp_thingmeta".into()));
        let repo = rig
            .repos
            .for_type(&thing)
            .expect("a table once the property is set");
        repo.add(ObjectId(3), &MetaKey::from("k"), None)
            .unwrap()
            .expect("an id");
        assert_eq!(count(&rig.db, "wp_thingmeta"), 1);
        assert!(Rc::ptr_eq(&repo, &rig.repos.for_type(&thing).unwrap()));
    });
}

/// A `query` callback's failure is `MetaError::Db` with its message; its payload stays for the bridge to take.
#[test]
fn hook_failure_surfaces() {
    on(Engine::Sqlite, |rig| {
        let repo = repo(&rig, "post");
        let _ = take_hook_error();
        rig.hooks
            .add_filter(
                &HookName::from("query"),
                Callback::rust("throws", |_: &[WpValue]| Err(HookError::message("boom"))),
                &WpValue::Int(10),
                1,
            )
            .unwrap();
        let failed = repo.count_for(ObjectId(1), &MetaKey::from("k"));
        assert!(
            matches!(failed, Err(MetaError::Db(ref m)) if m == "boom"),
            "{failed:?}"
        );
        assert_eq!(take_hook_error().map(|e| e.message), Some("boom".into()));
        // `$wpdb->insert`'s charset lookup meets the failure first and hides it ("Could not retrieve table
        // charset."); the write still fails, as WordPress would have thrown, and the payload stays for the bridge.
        let hidden = repo.add(ObjectId(1), &MetaKey::from("k"), None);
        assert!(
            matches!(hidden, Err(MetaError::Db(ref m)) if m == "boom"),
            "{hidden:?}"
        );
        assert!(take_hook_error().is_some());
        // So does every other write that answers 0 behind a hidden failure.
        let deleted = repo.delete_by_mid(MetaId(1));
        assert!(matches!(deleted, Err(MetaError::Db(_))), "{deleted:?}");
        assert!(take_hook_error().is_some());
    });
}

/// `add_metadata()` sends no formats, so `comment_id` (not in `$wpdb->field_types`) goes in as a string.
#[test]
fn comment_add_sends_comment_id_as_string() {
    on(Engine::Sqlite, |rig| {
        rig.db
            .query(&meta_table("wp_commentmeta", "comment_id", "meta_id"))
            .unwrap();
        let sent: Rc<RefCell<Vec<String>>> = Rc::default();
        let log = sent.clone();
        rig.hooks
            .add_filter(
                &HookName::from("query"),
                Callback::rust("record", move |args: &[WpValue]| {
                    let query = args.first().cloned().unwrap_or(WpValue::Null);
                    if let WpValue::Str(text) = &query {
                        log.borrow_mut().push(text.clone());
                    }
                    Ok(query)
                }),
                &WpValue::Int(10),
                1,
            )
            .unwrap();
        repo(&rig, "comment")
            .add(
                ObjectId(5),
                &MetaKey::from("k"),
                Some(&SerializedValue::new(b"v".to_vec())),
            )
            .unwrap()
            .expect("an id");
        let inserts: Vec<String> = sent
            .borrow()
            .iter()
            .filter(|q| q.starts_with("INSERT"))
            .cloned()
            .collect();
        assert_eq!(
            inserts,
            [
                "INSERT INTO `wp_commentmeta` (`comment_id`, `meta_key`, `meta_value`) VALUES ('5', 'k', 'v')"
            ]
        );
    });
}

/// `_get_meta_table( 'blog' )` is false on a single site: `$wpdb->blogmeta` is unset.
#[test]
fn single_site_has_no_blogmeta() {
    on(Engine::Sqlite, |rig| {
        assert!(rig.repos.for_type(&of("blog")).is_none());
    });
}

/// On multisite `set_prefix` sets `$wpdb->blogmeta`, so the type has its table.
#[test]
fn multisite_has_blogmeta() {
    on(Engine::Sqlite, |rig| {
        rig.host.multisite.replace(true);
        rig.db.set_prefix("wp_", true).unwrap();
        let repo = rig.repos.for_type(&of("blog")).expect("blogmeta is set");
        rig.db
            .query(&meta_table("wp_blogmeta", "blog_id", "meta_id"))
            .unwrap();
        repo.add(ObjectId(1), &MetaKey::from("k"), None)
            .unwrap()
            .expect("an id");
        assert_eq!(count(&rig.db, "wp_blogmeta"), 1);
    });
}
