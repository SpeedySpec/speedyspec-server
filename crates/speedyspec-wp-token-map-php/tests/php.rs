//! The token map in embedded PHP: WordPress's class and HTML5 table from the prepared release, replayed against
//! WordPress 7.1.5 (`compat/token-map/cases.json`, recorded by `compat/token-map/record.php`).
//!
//! PHP boots once per process, so this test has its own `main()` (`harness = false`).

use std::path::Path;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php, PhpError};
use speedyspec_wp_hooks::HookValue;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_token_map_php::{TokenMapArea, prepare_release};

fn check(name: &str, ok: bool, detail: impl std::fmt::Debug) {
    if ok {
        println!("ok: {name}");
    } else {
        eprintln!("FAIL: {name}: {detail:?}");
        std::process::exit(1);
    }
}

fn text(result: &Result<HookValue, PhpError>) -> String {
    match result {
        Ok(HookValue::Str(s)) => s.clone(),
        other => format!("{other:?}"),
    }
}

fn main() {
    let release = speedyspec_compat::wordpress_release("7.1.5").expect("WordPress 7.1.5");
    let prepared = prepare_release(
        &release,
        &release.with_file_name("wordpress-token-map-port"),
    )
    .expect("release prepared");
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container, Rc::new(MemoryHookStore::new()));
    let areas: Vec<Rc<dyn NativeArea>> = vec![Rc::new(hooks), Rc::new(TokenMapArea)];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let load = format!(
        "define('SPEEDYSPEC_TOKEN_MAP_ROOT', {:?}); require {:?};",
        prepared.display().to_string(),
        root.join("compat/token-map/load.php").display().to_string(),
    );
    php.eval("speedyspec://test/load", &load)
        .expect("WordPress's token map loads");

    let r = php.eval(
        "speedyspec://test/replay",
        &format!(
            "$r = require {:?}; return json_encode($r);",
            root.join("compat/token-map/replay.php")
                .display()
                .to_string()
        ),
    );
    let report: serde_json::Value =
        serde_json::from_str(&text(&r)).unwrap_or(serde_json::Value::Null);
    check(
        "recorded_calls_replay",
        report["calls"].as_i64().unwrap_or(0) > 100
            && report["mismatches"]
                .as_array()
                .is_some_and(|m| m.is_empty()),
        &report,
    );
}
