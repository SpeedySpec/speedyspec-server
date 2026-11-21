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
        "speedyspec://test/native",
        "global $html5_named_character_references;
         $m = $html5_named_character_references;
         $before = SpeedySpec\\WpTokenMap\\Native\\stats();
         $len = null; $a = $m->read_token('notin;x', 0, $len);
         $b = $m->contains('notin;');
         $after_html5 = SpeedySpec\\WpTokenMap\\Native\\stats() - $before;
         $own = WP_Token_Map::from_array(array('cat' => 'x'));
         $own->read_token('cat', 0); $own->contains('cat');
         $edge_len = 'unset'; @$m->read_token('a&', 2, $edge_len);
         $after_others = SpeedySpec\\WpTokenMap\\Native\\stats() - $before;
         return json_encode(array((new ReflectionClass('WP_Token_Map'))->getFileName(), $a, $len, $b, $after_html5, $after_others, $edge_len), JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE);",
    );
    check(
        "html5_table_reads_natively",
        text(&r) == r#"["speedyspec://wp-token-map","∉",6,true,2,2,"unset"]"#,
        &r,
    );

    let r = php.eval(
        "speedyspec://test/changed-table",
        "global $html5_named_character_references;
         $state = (function () { return array('storage_version' => '6.6.0-trunk', 'key_length' => $this->key_length, 'groups' => $this->groups,
             'large_words' => $this->large_words, 'small_words' => $this->small_words, 'small_mappings' => $this->small_mappings); })
             ->call($html5_named_character_references);
         $same = WP_Token_Map::from_precomputed_table($state);
         $state['small_mappings'][0] = 'changed';
         $changed = WP_Token_Map::from_precomputed_table($state);
         $before = SpeedySpec\\WpTokenMap\\Native\\stats();
         $changed->read_token('notin;', 0);
         $mid = SpeedySpec\\WpTokenMap\\Native\\stats() - $before;
         $same->read_token('notin;', 0);
         return json_encode(array($mid, SpeedySpec\\WpTokenMap\\Native\\stats() - $before));",
    );
    check("other_maps_run_wordpress_code", text(&r) == "[0,1]", &r);

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
