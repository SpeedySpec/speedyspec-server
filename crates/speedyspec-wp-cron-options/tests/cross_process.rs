//! Two processes (this binary run twice as `child_scheduler`) schedule events in the same MySQL `cron` option at the
//! same time; compare-and-swap keeps every event.

use std::collections::BTreeMap;
use std::rc::Rc;

use speedyspec_data::EngineConfig;
use speedyspec_data_mysql::MySqlEngineFactory;
use speedyspec_data_query::EngineRegistry;
use speedyspec_wp_cron::memory::{CollectingErrors, MemoryDoingCron, RecordingWake};
use speedyspec_wp_cron::{CronApi, CronParts, RustArgsKeyer};
use speedyspec_wp_cron_contracts::{Clock, CronResult, PhpObjects, Translator};
use speedyspec_wp_cron_options::OptionCronStore;
use speedyspec_wp_db::{MemoryWpdbHost, MemoryWpdbState, WpDb, WpDbConfig, WpDbParts};
use speedyspec_wp_hooks::{HookApi, HookValue, IgnoreDeprecations, Key};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_object_cache::{CurrentObjectCache, ObjectCachePorts};
use speedyspec_wp_options::{OptionsApi, OptionsDeps};
use speedyspec_wp_options_sql::SqlOptionRepository;

struct Now;
impl Clock for Now {
    fn microtime(&self) -> f64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64()
    }
    fn time(&self) -> i64 {
        self.microtime() as i64
    }
    fn pause(&self, millis: u64) {
        std::thread::sleep(std::time::Duration::from_millis(millis));
    }
}
struct Same;
impl Translator for Same {
    fn translate(&self, text: &str) -> String {
        text.into()
    }
}
struct Arrays;
impl PhpObjects for Arrays {
    fn to_object(&self, array: HookValue) -> HookValue {
        array
    }
    fn property(&self, object: &HookValue, name: &str) -> Option<HookValue> {
        match object {
            HookValue::Array(items) => items
                .iter()
                .find(|(k, _)| *k == Key::Str(name.into()))
                .map(|(_, v)| v.clone()),
            _ => None,
        }
    }
    fn is_wp_error(&self, _: &HookValue) -> bool {
        false
    }
}

fn db(prefix: &str) -> Option<Rc<WpDb>> {
    let url = std::env::var("SPEEDYSPEC_TEST_MYSQL_URL").ok()?;
    let name = url.rsplit('/').next().unwrap_or_default().to_string();
    let registry = EngineRegistry::new();
    registry.register_factory(Rc::new(MySqlEngineFactory));
    registry
        .configure(EngineConfig {
            name: "default".into(),
            url,
            options: BTreeMap::new(),
        })
        .unwrap();
    let db = Rc::new(WpDb::new(WpDbParts {
        store: registry.sql("default").unwrap(),
        hooks: Rc::new(HookApi::new(
            Rc::new(MemoryHookStore::new()),
            Rc::new(IgnoreDeprecations),
        )),
        state: Rc::new(MemoryWpdbState::new()),
        host: Rc::new(MemoryWpdbHost::new()),
        config: WpDbConfig {
            user: String::new(),
            password: String::new(),
            name,
            host: "localhost".into(),
            charset: Some("utf8mb4".into()),
            collate: None,
            table_prefix: prefix.into(),
        },
    }));
    db.db_connect(true).unwrap();
    db.set_prefix(prefix, true).unwrap();
    Some(db)
}

/// A process's cron API over its own options API and object cache.
fn cron_api(db: Rc<WpDb>) -> CronApi {
    let hooks = Rc::new(HookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let cache = Rc::new(CurrentObjectCache::new(ObjectCachePorts::rust(), None));
    let repo = Rc::new(SqlOptionRepository::new(db, "default".into()));
    let options = Rc::new(OptionsApi::new(OptionsDeps::rust(
        hooks.clone(),
        cache,
        repo,
    )));
    CronApi::new(CronParts {
        hooks,
        store: Rc::new(OptionCronStore::new(options)),
        keyer: Rc::new(RustArgsKeyer),
        clock: Rc::new(Now),
        translator: Rc::new(Same),
        objects: Rc::new(Arrays),
        wake: Rc::new(RecordingWake::new()),
        doing_cron: Rc::new(MemoryDoingCron::new()),
        errors: Rc::new(CollectingErrors::new()),
    })
}

fn fresh_table(db: &WpDb, prefix: &str) {
    db.query(&format!("DROP TABLE IF EXISTS {prefix}options"))
        .unwrap();
    db.query(&format!(
        "CREATE TABLE {prefix}options (option_id bigint(20) unsigned NOT NULL auto_increment, \
         option_name varchar(191) NOT NULL default '', option_value longtext NOT NULL, \
         autoload varchar(20) NOT NULL default 'yes', PRIMARY KEY (option_id), UNIQUE KEY option_name (option_name), \
         KEY autoload (autoload)) DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci"
    ))
    .unwrap();
}

fn hooks_in(api: &CronApi) -> usize {
    api.get_cron_array().unwrap().events().len()
}

fn schedule(api: &CronApi, hook: &str, offset: i64) -> CronResult {
    api.schedule_single_event(
        &HookValue::Int(Now.time() + 3600 + offset),
        &HookValue::Str(hook.into()),
        &HookValue::Array(vec![]),
        true,
    )
    .unwrap()
}

#[test]
fn child_scheduler() {
    let Ok(prefix) = std::env::var("SPEEDYSPEC_CRON_CHILD") else {
        return;
    };
    let api = cron_api(db("cronx_").unwrap());
    for i in 0..50 {
        // Separate requests, not a tight loop: a writer that never pauses can starve another past its retries.
        std::thread::sleep(std::time::Duration::from_millis(10));
        let result = schedule(&api, &format!("{prefix}_{i}"), i);
        assert_eq!(
            result,
            CronResult::Value(HookValue::Bool(true)),
            "{prefix}_{i}"
        );
    }
}

#[test]
fn two_processes_schedule_and_both_survive() {
    let Some(db) = db("cronx_") else {
        println!("skip: SPEEDYSPEC_TEST_MYSQL_URL is not set");
        return;
    };
    fresh_table(&db, "cronx_");
    let api = cron_api(db.clone());
    api.set_cron_array(&HookValue::Array(vec![]), true).unwrap();
    let children: Vec<_> = ["one", "two"]
        .iter()
        .map(|prefix| {
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "child_scheduler", "--nocapture"])
                .env("SPEEDYSPEC_CRON_CHILD", prefix)
                .spawn()
                .unwrap()
        })
        .collect();
    for mut child in children {
        assert!(child.wait().unwrap().success());
    }
    assert_eq!(hooks_in(&cron_api(db)), 100);
}

#[test]
fn stale_cache_never_overwrites() {
    // Its own table, so it never races the two-process test.
    let Some(db) = db("cronstale_") else {
        println!("skip: SPEEDYSPEC_TEST_MYSQL_URL is not set");
        return;
    };
    fresh_table(&db, "cronstale_");
    let a = cron_api(db.clone());
    let b = cron_api(db.clone());
    a.set_cron_array(&HookValue::Array(vec![]), true).unwrap();
    // `a` reads (and caches) the option; `b` then writes behind `a`'s cache.
    assert_eq!(hooks_in(&a), 0);
    assert_eq!(
        schedule(&b, "from_b", 0),
        CronResult::Value(HookValue::Bool(true))
    );
    assert_eq!(
        schedule(&a, "from_a", 10_000),
        CronResult::Value(HookValue::Bool(true))
    );
    assert_eq!(hooks_in(&cron_api(db)), 2);
}
