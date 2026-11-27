//! Boots PHP with the native areas post reading runs beside (the Hook API, `wpdb`, the object cache, the meta
//! functions and the post functions), over a MySQL database of the test's own, and prepares the release they run
//! over: meta-php's preparation, then this crate's, in one copy.
//!
//! Each test binary gets its own database (`<database>_<suffix>`, made when missing), because
//! compat/posts-read/setup.php empties every table it seeds.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_data::EngineConfig;
use speedyspec_data_mysql::MySqlEngineFactory;
use speedyspec_data_query::{EngineRegistry, parse_one};
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php};
use speedyspec_php_ext::{Value, twins_area};
use speedyspec_wp_db::WpDbConfig;
use speedyspec_wp_db_php::WpDbArea;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_meta_php::MetaArea;
use speedyspec_wp_object_cache_php::ObjectCacheArea;
use speedyspec_wp_posts_php::PostsArea;

/// `url` with its database replaced by `<database>_<suffix>`, which is made when missing.
fn own_database(url: &str, suffix: &str) -> String {
    let (base, name) = url.rsplit_once('/').expect("the URL names a database");
    let registry = EngineRegistry::new();
    registry.register_factory(Rc::new(MySqlEngineFactory));
    registry
        .configure(EngineConfig {
            name: "default".into(),
            url: url.to_owned(),
            options: BTreeMap::new(),
        })
        .unwrap();
    let statement = parse_one(&format!("CREATE DATABASE IF NOT EXISTS {name}_{suffix}")).unwrap();
    registry
        .sql("default")
        .and_then(|sql| sql.execute(&statement))
        .unwrap();
    format!("{base}/{name}_{suffix}")
}

/// The MySQL URL of the test's own database, `None` (and a skip notice) without `SPEEDYSPEC_TEST_MYSQL_URL`.
pub fn mysql(suffix: &str) -> Option<String> {
    let Ok(url) = std::env::var("SPEEDYSPEC_TEST_MYSQL_URL") else {
        println!("skipped: needs MySQL: set SPEEDYSPEC_TEST_MYSQL_URL");
        return None;
    };
    let own = own_database(&url, suffix);
    // compat/posts-read/setup.php connects through this variable.
    // SAFETY: no other thread runs yet.
    unsafe { std::env::set_var("SPEEDYSPEC_TEST_MYSQL_URL", &own) };
    Some(own)
}

/// The pinned release, prepared in `<release>-<name>` for the native meta and post functions.
pub fn prepared(name: &str) -> (PathBuf, PathBuf) {
    let release =
        speedyspec_compat::wordpress_release(&speedyspec_compat::area_wordpress("posts-read"))
            .expect("the pinned release");
    let out = release.with_file_name(format!("wordpress-{name}"));
    speedyspec_wp_meta_php::prepare_release(&release, &out).expect("meta.php prepares");
    let root = speedyspec_wp_posts_php::prepare_release(&release, &out).expect("post.php prepares");
    (release, root)
}

/// Boots PHP with the twins, the container (with the meta and post ports), the Hook API, `wpdb`, the object cache,
/// the meta area and the post area, over `url`.
pub fn boot(url: &str) -> (Php, Rc<Container>) {
    boot_with(url, false)
}

/// As [`boot`], with the Hook API's testing class (`SpeedySpec\WpHooks\Testing`, the snapshots WordPress's test case
/// stand-in takes) when `testing` is set.
pub fn boot_with(url: &str, testing: bool) -> (Php, Rc<Container>) {
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
    let hooks = if testing { hooks.with_testing() } else { hooks };
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
    let posts = PostsArea::new(
        container.clone(),
        hooks.api(),
        cache.api(),
        wpdb.api(),
        meta.api(),
    );
    let ports = PostsArea::ports(MetaArea::ports(ContainerArea::new(
        "tests",
        container.clone(),
    )));
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        twins_area(),
        Rc::new(ports),
        Rc::new(hooks),
        Rc::new(wpdb),
        Rc::new(cache),
        Rc::new(meta),
        Rc::new(posts),
    ];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");
    (php, container)
}

/// The code that loads compat/posts-read/setup.php (single site) over `root`: WordPress's own files, the seeded
/// tables, and the core post types.
pub fn setup_code(root: &Path) -> String {
    use speedyspec_wp_posts_php::php_string;
    format!(
        "define('SPEEDYSPEC_COMPAT_WP_ROOT', {}); define('SPEEDYSPEC_COMPAT_PORT', true); \
         define('SPEEDYSPEC_COMPAT_MULTISITE', false); require {}; return true;",
        php_string(&format!("{}/", root.display())),
        php_string(
            &speedyspec_compat::compat_dir()
                .join("posts-read/setup.php")
                .display()
                .to_string()
        )
    )
}

pub fn check(name: &str, ok: bool, detail: impl std::fmt::Debug) -> bool {
    if ok {
        println!("ok: {name}");
    } else {
        eprintln!("FAIL: {name}: {detail:?}");
    }
    ok
}

/// Runs `code` in a fresh request with `setup` loaded; passes when it returns `true`.
pub fn yes(php: &Php, setup: &str, name: &str, code: &str) -> bool {
    php.restart_request().unwrap();
    if let Err(e) = php.eval("speedyspec://posts-read/setup", setup) {
        return check(name, false, e);
    }
    let r = php.eval("speedyspec://posts-read/case", code);
    check(name, matches!(r, Ok(Value::Bool(true))), r)
}
