//! A persistent second level shared across processes: a child process (this test binary, run as `child_writer`)
//! writes options; the parent, in a new request, reads what the child wrote through the shared second level.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;
use std::time::Duration;

use speedyspec_cache::CachingStore;
use speedyspec_cache_file::FileCache;
use speedyspec_cache_kv::KvCache;
use speedyspec_cache_sql::SqlCache;
use speedyspec_data::{DirectStore, EngineConfig};
use speedyspec_data_file::FileStore;
use speedyspec_data_mysql::MySqlEngineFactory;
use speedyspec_data_query::EngineRegistry;
use speedyspec_data_valkey::{ValkeyStore, driver_url};
use speedyspec_wp_db::{MemoryWpdbHost, MemoryWpdbState, WpDb, WpDbConfig, WpDbParts};
use speedyspec_wp_hooks::{HookApi, IgnoreDeprecations};
use speedyspec_wp_hooks_contracts::{Callback, HookName, HookValue, Key};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_object_cache::{CurrentObjectCache, ObjectCachePorts};
use speedyspec_wp_options::{
    ExternalObjectCache, FixedProtectedOptions, HookSanitizing, InstallingFlag, NetworkOptionsApi,
    NetworkPorts, OptionsApi, OptionsDeps, SiteConfiguration, SystemClock, TransientPorts,
    TransientsApi,
};
use speedyspec_wp_options_sql::{SqlExpiredTransients, SqlOptionRepository, SqlSitemetaRepository};

/// The backends share one `wp_options`: one at a time.
static TABLE: Mutex<()> = Mutex::new(());

const SCHEMA: &str = "CREATE TABLE wp_options (option_id bigint(20) unsigned NOT NULL auto_increment, \
    option_name varchar(191) NOT NULL default '', option_value longtext NOT NULL, \
    autoload varchar(20) NOT NULL default 'yes', PRIMARY KEY (option_id), UNIQUE KEY option_name (option_name), \
    KEY autoload (autoload)) DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci";

struct Rig {
    api: Rc<OptionsApi>,
    transients: TransientsApi,
    current: Rc<CurrentObjectCache>,
    queries: Rc<Cell<u32>>,
}

/// A site with a persistent object cache, as `wp_using_ext_object_cache()` answers when the runtime has one.
struct External;

impl ExternalObjectCache for External {
    fn using(&self) -> bool {
        true
    }
}

fn s(v: &str) -> HookValue {
    HookValue::Str(v.into())
}

/// A request's options API over MySQL and the backend's second level; `fresh` recreates `wp_options` and empties
/// the second level.
fn rig(backend: &str, dir: &Path, fresh: bool) -> Rig {
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
    let sql = registry.sql("default").unwrap();
    let db = Rc::new(WpDb::new(WpDbParts {
        store: sql.clone(),
        hooks: hooks.clone(),
        state: Rc::new(MemoryWpdbState::new()),
        host: Rc::new(MemoryWpdbHost::new()),
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
    db.set_prefix("wp_", true).unwrap();
    if fresh {
        db.query("DROP TABLE IF EXISTS wp_options").unwrap();
        db.query(SCHEMA).unwrap();
    }
    let store: Rc<dyn CachingStore> = match backend {
        "sql" => {
            let cache = SqlCache::new(sql, "wp_speedyspec_cache").unwrap();
            if fresh {
                cache.install().unwrap();
            }
            Rc::new(cache)
        }
        "file" => {
            let files = FileStore::new("files", dir.to_path_buf());
            files.connect().unwrap();
            Rc::new(FileCache::new(&files, "object-cache").unwrap())
        }
        "valkey" => {
            let url = std::env::var("SPEEDYSPEC_TEST_VALKEY_URL")
                .expect("set SPEEDYSPEC_TEST_VALKEY_URL");
            let prefix = format!(
                "speedyspec-cross-{}:",
                dir.file_name().unwrap().to_string_lossy()
            );
            let kv = ValkeyStore::new(
                "valkey",
                &driver_url(&url, false).unwrap(),
                &prefix,
                Duration::from_secs(2),
            )
            .unwrap();
            Rc::new(KvCache::new(Rc::new(kv), "object-cache"))
        }
        other => panic!("unknown backend {other}"),
    };
    if fresh {
        store.flush().unwrap();
    }
    let current = Rc::new(CurrentObjectCache::new(
        ObjectCachePorts::rust(),
        Some(store),
    ));
    let repo = Rc::new(SqlOptionRepository::new(db.clone(), "default".into()));
    let api = Rc::new(OptionsApi::new(OptionsDeps::rust(
        hooks.clone(),
        current.clone(),
        repo,
    )));
    let site = Rc::new(SiteConfiguration {
        multisite: false,
        blog_id: 1,
        network_id: 1,
    });
    let installing = Rc::new(InstallingFlag::new());
    let network = Rc::new(NetworkOptionsApi::new(
        api.clone(),
        Rc::new(SqlSitemetaRepository::new(db.clone())),
        current.clone(),
        hooks.clone(),
        NetworkPorts {
            multisite: site.clone(),
            installing: installing.clone(),
            sanitizing: Rc::new(HookSanitizing::new(hooks.clone())),
            protected: Rc::new(FixedProtectedOptions),
        },
    ));
    let transients = TransientsApi::new(
        api.clone(),
        network,
        current.clone(),
        hooks.clone(),
        TransientPorts {
            external: Rc::new(External),
            installing,
            multisite: site,
            clock: Rc::new(SystemClock),
            expired: Rc::new(SqlExpiredTransients::new(db)),
        },
    );
    let queries = Rc::new(Cell::new(0));
    let counter = queries.clone();
    hooks
        .add_filter(
            &HookName::from("query"),
            Callback::rust("count_queries", move |args: &[HookValue]| {
                counter.set(counter.get() + 1);
                Ok(args[0].clone())
            }),
            &HookValue::Int(10),
            1,
        )
        .unwrap();
    Rig {
        api,
        transients,
        current,
        queries,
    }
}

/// Runs this binary's `child_writer` for `step`, which must exit 0.
fn child(backend: &str, dir: &Path, step: &str) {
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_writer", "--ignored", "--nocapture"])
        .env("SPEEDYSPEC_CROSS_ROLE", step)
        .env("SPEEDYSPEC_CROSS_BACKEND", backend)
        .env("SPEEDYSPEC_CROSS_DIR", dir)
        .status()
        .unwrap();
    assert!(status.success(), "child step {step} on {backend}: {status}");
}

/// The child's side of each step: one write in its own process.
#[test]
#[ignore = "run by the cross-process tests"]
fn child_writer() {
    let Ok(step) = std::env::var("SPEEDYSPEC_CROSS_ROLE") else {
        return;
    };
    let backend = std::env::var("SPEEDYSPEC_CROSS_BACKEND").unwrap();
    let dir = PathBuf::from(std::env::var("SPEEDYSPEC_CROSS_DIR").unwrap());
    let r = rig(&backend, &dir, false);
    let ok = match step.as_str() {
        "update" => r
            .api
            .update_option("ss_cross", s("two"), HookValue::Null)
            .unwrap(),
        "add_missing" => r
            .api
            .add_option("ss_missing", s("here"), s(""), s("off"))
            .unwrap(),
        "add_auto" => r.api.add_option("ss_auto", s("x"), s(""), s("on")).unwrap(),
        "delete" => r.api.delete_option("ss_cross").unwrap(),
        "transients" => {
            let hour = HookValue::Int(3600);
            r.transients.set_transient("ss_t", s("tv"), &hour).unwrap()
                && r.transients
                    .set_site_transient("ss_st", s("sv"), &hour)
                    .unwrap()
                && r.transients
                    .set_transient("ss_short", s("x"), &HookValue::Int(1))
                    .unwrap()
        }
        other => panic!("unknown step {other}"),
    };
    assert!(ok, "{step} wrote nothing");
}

fn cross_process(backend: &str) {
    let _one = TABLE.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let r = rig(backend, dir.path(), true);

    // update_option_seen_at_once
    assert!(
        r.api
            .add_option("ss_cross", s("one"), s(""), s("on"))
            .unwrap()
    );
    assert_eq!(r.api.get_option("ss_cross", None).unwrap(), s("one"));
    child(backend, dir.path(), "update");
    assert_eq!(
        r.api.get_option("ss_cross", None).unwrap(),
        s("one"),
        "{backend}: the first level still holds it"
    );
    r.current.end_request();
    r.queries.set(0);
    assert_eq!(
        r.api.get_option("ss_cross", None).unwrap(),
        s("two"),
        "{backend}: update_option_seen_at_once"
    );
    assert_eq!(
        r.queries.get(),
        0,
        "{backend}: update_option_seen_at_once read SQL"
    );

    // notoptions_invalidated
    assert_eq!(
        r.api.get_option("ss_missing", None).unwrap(),
        HookValue::Bool(false)
    );
    child(backend, dir.path(), "add_missing");
    r.current.end_request();
    assert_eq!(
        r.api.get_option("ss_missing", None).unwrap(),
        s("here"),
        "{backend}: notoptions_invalidated"
    );

    // alloptions_invalidated
    child(backend, dir.path(), "add_auto");
    r.current.end_request();
    let all = r.api.load_alloptions(false).unwrap();
    let HookValue::Array(entries) = &all else {
        panic!("{all:?}")
    };
    assert!(
        entries
            .iter()
            .any(|(k, v)| *k == Key::Str("ss_auto".into()) && *v == s("x")),
        "{backend}: alloptions_invalidated: {all:?}"
    );

    // transients_shared: with a persistent object cache, a transient another process set is read from the second
    // level (no SQL, no wp_options row), and its expiry travels with it.
    child(backend, dir.path(), "transients");
    r.current.end_request();
    r.queries.set(0);
    assert_eq!(
        r.transients.get_transient("ss_t").unwrap(),
        s("tv"),
        "{backend}: transients_shared"
    );
    assert_eq!(
        r.transients.get_site_transient("ss_st").unwrap(),
        s("sv"),
        "{backend}: site transient shared"
    );
    assert_eq!(r.queries.get(), 0, "{backend}: transients_shared read SQL");
    assert_eq!(
        r.api.get_option("_transient_ss_t", None).unwrap(),
        HookValue::Bool(false),
        "{backend}: the transient was written to wp_options"
    );
    std::thread::sleep(Duration::from_millis(2100));
    r.current.end_request();
    assert_eq!(
        r.transients.get_transient("ss_short").unwrap(),
        HookValue::Bool(false),
        "{backend}: the expiry did not reach the second level"
    );

    // delete_seen
    child(backend, dir.path(), "delete");
    r.current.end_request();
    assert_eq!(
        r.api.get_option("ss_cross", None).unwrap(),
        HookValue::Bool(false),
        "{backend}: delete_seen"
    );
}

#[test]
#[ignore = "needs MySQL: set SPEEDYSPEC_TEST_MYSQL_URL"]
fn cross_process_sql() {
    cross_process("sql");
}

#[test]
#[ignore = "needs MySQL: set SPEEDYSPEC_TEST_MYSQL_URL"]
fn cross_process_file() {
    cross_process("file");
}

#[test]
#[ignore = "needs MySQL and Valkey: set SPEEDYSPEC_TEST_MYSQL_URL and SPEEDYSPEC_TEST_VALKEY_URL"]
fn cross_process_valkey() {
    cross_process("valkey");
}
