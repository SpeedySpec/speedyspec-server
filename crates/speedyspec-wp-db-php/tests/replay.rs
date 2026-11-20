//! WordPress's `wpdb`, recorded on MySQL (`compat/wp-db`), replayed exactly through the native class.
//!
//! Needs MySQL (`SPEEDYSPEC_TEST_MYSQL_URL`); without it the test reports that it was skipped.

use std::collections::BTreeMap;
use std::rc::Rc;

use speedyspec_compat::{PhpRunner, compat_dir, replay};
use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_data::EngineConfig;
use speedyspec_data_mysql::MySqlEngineFactory;
use speedyspec_data_query::EngineRegistry;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php};
use speedyspec_php_ext::twins_area;
use speedyspec_wp_db::WpDbConfig;
use speedyspec_wp_db_php::WpDbArea;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;

fn main() {
    let Ok(url) = std::env::var("SPEEDYSPEC_TEST_MYSQL_URL") else {
        println!("skipped: needs MySQL: set SPEEDYSPEC_TEST_MYSQL_URL");
        return;
    };
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
    let registry = Rc::new(EngineRegistry::new());
    registry.register_factory(Rc::new(MySqlEngineFactory));
    registry
        .configure(EngineConfig {
            name: "default".into(),
            url: url.clone(),
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
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        twins_area(),
        Rc::new(ContainerArea::new("demo", container)),
        Rc::new(hooks),
        Rc::new(wpdb),
    ];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");
    let support = compat_dir().join("wp-db/support.php");
    let mut runner = PhpRunner::new(php, move |php: &Php| {
        php.require(&support).map(|_| ()).map_err(|e| e.to_string())
    });
    let mismatches = replay("wp-db", &mut runner);
    for m in &mismatches {
        eprintln!("MISMATCH {m}");
        eprintln!(
            "  expected: {}",
            m.expected
                .as_ref()
                .map(|e| serde_json::to_string(e).unwrap_or_default())
                .unwrap_or_default()
        );
        eprintln!(
            "  actual:   {}",
            m.actual
                .as_ref()
                .map(|e| serde_json::to_string(e).unwrap_or_default())
                .unwrap_or_default()
        );
    }
    if !mismatches.is_empty() {
        eprintln!("{} scenario(s) differ from WordPress", mismatches.len());
        std::process::exit(1);
    }
    println!("ok: wp_db_replays_exactly");
}
