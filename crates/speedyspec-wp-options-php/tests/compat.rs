//! WordPress's options functions, recorded on MySQL (`compat/options`, single site and multisite), replayed exactly
//! through the native ones.
//!
//! Needs MySQL (`SPEEDYSPEC_TEST_MYSQL_URL`); without it the test reports that it was skipped.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;

use speedyspec_compat::{replay_for, wordpress_release};
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
use speedyspec_wp_object_cache_php::ObjectCacheArea;
use speedyspec_wp_options_php::{OptionsArea, options_scenario_runner};

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
    let cache = ObjectCacheArea::new(container.clone(), None);
    let options = OptionsArea::new(container.clone(), hooks.api(), cache.api(), wpdb.api());
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        twins_area(),
        Rc::new(ContainerArea::new("compat", container)),
        Rc::new(hooks),
        Rc::new(wpdb),
        Rc::new(cache),
        Rc::new(options),
    ];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");
    let release = wordpress_release("7.1.5").expect("the pinned release");
    let multisite = Rc::new(Cell::new(false));
    let mut runner =
        options_scenario_runner(php, &release, multisite.clone()).expect("the release prepares");
    // Both fixtures: the single-site scenarios, then those recorded on multisite, on a multisite port.
    let mut mismatches = Vec::new();
    for network in [false, true] {
        multisite.set(network);
        // SPEEDYSPEC_COMPAT_TRACE=1 replays one scenario at a time, naming each and its time.
        if std::env::var_os("SPEEDYSPEC_COMPAT_TRACE").is_some() {
            let (catalogue, fixture, breaks) =
                speedyspec_compat::load_for("options", network).expect("the options files load");
            let only = std::env::var("SPEEDYSPEC_COMPAT_ONLY").ok();
            for scenario in catalogue
                .scenarios
                .iter()
                .filter(|s| only.as_ref().is_none_or(|o| *o == s.name))
            {
                let one = speedyspec_compat::Catalogue {
                    area: catalogue.area.clone(),
                    scenarios: vec![scenario.clone()],
                };
                let started = std::time::Instant::now();
                eprintln!("trace: {} ...", scenario.name);
                mismatches.extend(speedyspec_compat::replay_with(
                    &one,
                    &fixture,
                    &breaks,
                    &mut runner,
                ));
                eprintln!("trace: {} {:?}", scenario.name, started.elapsed());
            }
        } else {
            mismatches.extend(replay_for("options", network, &mut runner));
        }
    }
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
        eprintln!(
            "FAIL: options_replay_matches: {} scenarios differ",
            mismatches.len()
        );
        std::process::exit(1);
    }
    println!("ok: options_replay_matches");
}
