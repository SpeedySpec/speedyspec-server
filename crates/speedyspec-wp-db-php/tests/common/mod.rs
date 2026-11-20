//! Shared by the replays: the PHP that runs `compat/wp-db` over a chosen engine, and an engine's breaks.

#![allow(dead_code)]

use std::rc::Rc;

use speedyspec_compat::{Break, Breaks, PhpRunner, compat_dir, load};
use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_data::{EngineConfig, EngineFactory};
use speedyspec_data_query::EngineRegistry;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php};
use speedyspec_php_ext::twins_area;
use speedyspec_wp_db::WpDbConfig;
use speedyspec_wp_db_php::WpDbArea;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;

/// `breaks.json`'s shared `breaks`, plus `engine_breaks[engine]`.
pub fn breaks_for_engine(area: &str, engine: &str) -> Breaks {
    let (_, _, mut breaks) = load(area).expect("the compatibility files load");
    let path = compat_dir().join(area).join("breaks.json");
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    if let Some(extra) = raw.get("engine_breaks").and_then(|e| e.get(engine)) {
        let extra: Vec<Break> =
            serde_json::from_value(extra.clone()).expect("engine_breaks entries");
        breaks.breaks.extend(extra);
    }
    breaks
}

/// The scenario names `engine_breaks[engine]` lists.
pub fn engine_break_scenarios(area: &str, engine: &str) -> Vec<String> {
    let path = compat_dir().join(area).join("breaks.json");
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    raw.get("engine_breaks")
        .and_then(|e| e.get(engine))
        .and_then(|e| e.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|b| b["scenario"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The `wp-db` scenarios' PHP over the `default` engine `config`, built by `factory`.
pub fn wp_db_runner(
    factory: Rc<dyn EngineFactory>,
    config: EngineConfig,
    wpdb: WpDbConfig,
) -> PhpRunner {
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
    let registry = Rc::new(EngineRegistry::new());
    registry.register_factory(factory);
    registry.configure(config).unwrap();
    let wpdb = WpDbArea::new(container.clone(), hooks.api(), registry, wpdb);
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
    PhpRunner::new(php, move |php: &Php| {
        php.require(&support).map(|_| ()).map_err(|e| e.to_string())
    })
}
