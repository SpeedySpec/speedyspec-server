//! Boots PHP with the native areas the meta functions run beside, over a MySQL database of the test's own.
//!
//! Each test binary gets its own database (`<database>_<suffix>`, made when missing), so the replays and the PHP tests
//! never empty each other's tables, nor compat/options' (compat/metadata/setup.php truncates `wp_options`).

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_data::EngineConfig;
use speedyspec_data_mysql::MySqlEngineFactory;
use speedyspec_data_query::{EngineRegistry, parse_one};
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php, PhpError};
use speedyspec_php_ext::{Value, twins_area};
use speedyspec_wp_db::WpDbConfig;
use speedyspec_wp_db_php::WpDbArea;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_meta_php::MetaArea;
use speedyspec_wp_object_cache_php::ObjectCacheArea;

pub struct Booted {
    pub php: Php,
    pub container: Rc<Container>,
    /// The database URL the test runs on.
    pub url: String,
}

/// `url` with its database replaced by `<database>_<suffix>`, which is made when missing.
fn own_database(url: &str, suffix: &str) -> Result<String, String> {
    let (base, name) = url.rsplit_once('/').ok_or("the URL names no database")?;
    let own = format!("{base}/{name}_{suffix}");
    let registry = EngineRegistry::new();
    registry.register_factory(Rc::new(MySqlEngineFactory));
    registry
        .configure(EngineConfig {
            name: "default".into(),
            url: url.to_owned(),
            options: BTreeMap::new(),
        })
        .map_err(|e| e.to_string())?;
    let statement = parse_one(&format!("CREATE DATABASE IF NOT EXISTS {name}_{suffix}"))
        .map_err(|e| e.to_string())?;
    registry
        .sql("default")
        .and_then(|sql| sql.execute(&statement))
        .map_err(|e| e.to_string())?;
    Ok(own)
}

/// The MySQL URL of the test's own database, `None` (and a skip notice) without `SPEEDYSPEC_TEST_MYSQL_URL`.
pub fn mysql(suffix: &str) -> Option<String> {
    let Ok(url) = std::env::var("SPEEDYSPEC_TEST_MYSQL_URL") else {
        println!("skipped: needs MySQL: set SPEEDYSPEC_TEST_MYSQL_URL");
        return None;
    };
    let own =
        own_database(&url, suffix).unwrap_or_else(|e| panic!("making the test database: {e}"));
    // compat/metadata/setup.php connects through this variable.
    // SAFETY: no other thread runs yet.
    unsafe { std::env::set_var("SPEEDYSPEC_TEST_MYSQL_URL", &own) };
    Some(own)
}

/// Boots PHP with the twins, the container (with the meta repositories port), the Hook API, `wpdb`, the object cache
/// and the meta area, over `url`.
pub fn boot(url: &str) -> Booted {
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
    let registry = Rc::new(EngineRegistry::new());
    registry.register_factory(Rc::new(MySqlEngineFactory));
    registry
        .configure(EngineConfig {
            name: "default".into(),
            url: url.to_owned(),
            options: BTreeMap::new(),
        })
        .unwrap();
    let rest = url.trim_start_matches("mysql://");
    let (auth, host_db) = rest.split_once('@').expect("user@host");
    let (user, password) = auth.split_once(':').unwrap_or((auth, ""));
    let (host, name) = host_db.split_once('/').expect("host/database");
    let config = WpDbConfig {
        user: user.into(),
        password: password.into(),
        name: name.into(),
        host: host.into(),
        charset: Some("utf8mb4".into()),
        collate: Some(String::new()),
        table_prefix: "wp_".into(),
    };
    let wpdb = WpDbArea::new(container.clone(), hooks.api(), registry, config);
    let cache = ObjectCacheArea::new(container.clone(), None);
    let meta = MetaArea::new(container.clone(), hooks.api(), cache.api(), wpdb.api());
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        twins_area(),
        Rc::new(MetaArea::ports(ContainerArea::new(
            "tests",
            container.clone(),
        ))),
        Rc::new(hooks),
        Rc::new(wpdb),
        Rc::new(cache),
        Rc::new(meta),
    ];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");
    Booted {
        php,
        container,
        url: url.to_owned(),
    }
}

pub fn eval(code: &str) -> Result<Value, PhpError> {
    speedyspec_php::eval("case", code)
}

pub fn check(name: &str, ok: bool, detail: impl std::fmt::Debug) {
    if ok {
        println!("ok: {name}");
    } else {
        eprintln!("FAIL: {name}: {detail:?}");
        std::process::exit(1);
    }
}

/// Runs `code`; the test passes when it returns `true`, and otherwise reports what it returned.
pub fn yes(name: &str, code: &str) {
    let r = eval(code);
    check(name, matches!(r, Ok(Value::Bool(true))), r);
}

/// The WordPress functions the meta functions call, as WordPress has them (functions.php is not loaded), and a post
/// meta table emptied for the test.
pub const WORDPRESS: &str = r#"
function wp_installing($is_installing = null) { return false; }
function is_multisite() { return false; }
function __($text, $domain = 'default') { return $text; }
function _doing_it_wrong($function_name, $message, $version) {
    do_action('doing_it_wrong_run', $function_name, $message, $version);
}
function _deprecated_function($function_name, $version, $replacement = '') {}
function get_post_type($post = null) { if (isset($GLOBALS['speedyspec_throw'])) { throw $GLOBALS['speedyspec_throw']; } return 'post'; }
function wp_unslash($value) { return $value; }
global $wpdb;
$wpdb->set_prefix('wp_');
wp_cache_init();
$wpdb->query('DROP TABLE IF EXISTS wp_postmeta');
$wpdb->query("CREATE TABLE wp_postmeta (meta_id bigint(20) unsigned NOT NULL auto_increment, post_id bigint(20) unsigned NOT NULL default '0', meta_key varchar(255) default NULL, meta_value longtext, PRIMARY KEY (meta_id), KEY post_id (post_id), KEY meta_key (meta_key(191))) DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci");
return true;
"#;
