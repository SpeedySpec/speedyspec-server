//! The native option functions from PHP, over MySQL. PHP boots once per process (`harness = false`).

use std::collections::BTreeMap;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_data::EngineConfig;
use speedyspec_data_mysql::MySqlEngineFactory;
use speedyspec_data_query::EngineRegistry;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php, PhpError};
use speedyspec_php_ext::{Value, boundary_crossings, twins_area};
use speedyspec_wp_db::WpDbConfig;
use speedyspec_wp_db_php::WpDbArea;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_object_cache_php::ObjectCacheArea;
use speedyspec_wp_options_php::OptionsArea;

fn check(name: &str, ok: bool, detail: impl std::fmt::Debug) {
    if ok {
        println!("ok: {name}");
    } else {
        eprintln!("FAIL: {name}: {detail:?}");
        std::process::exit(1);
    }
}

fn eval(code: &str) -> Result<Value, PhpError> {
    speedyspec_php::eval("case", code)
}

fn yes(name: &str, code: &str) {
    let r = eval(code);
    check(name, matches!(r, Ok(Value::Bool(true))), r);
}

/// The few WordPress functions the option functions call, as WordPress has them (functions.php is not loaded).
const WORDPRESS: &str = r#"
function wp_installing($is_installing = null) {
    static $installing = false;
    if (null !== $is_installing) { $old = $installing; $installing = (bool) $is_installing; return $old; }
    return $installing;
}
function is_multisite() { return false; }
function get_current_blog_id() { return 1; }
function get_current_network_id() { return 1; }
function __($text, $domain = 'default') { return $text; }
function esc_html($text) { return htmlspecialchars((string) $text, ENT_QUOTES); }
function esc_attr($text) { return htmlspecialchars((string) $text, ENT_QUOTES); }
function _deprecated_argument($function_name, $version, $message = '') {
    do_action('deprecated_argument_run', $function_name, $message, $version);
}
function wp_die($message = '') {
    $handler = apply_filters('wp_die_handler', '_default_wp_die_handler');
    call_user_func($handler, $message);
}
function _default_wp_die_handler($message) { throw new Error('wp_die: ' . $message); }
return true;
"#;

fn main() {
    let Ok(url) = std::env::var("SPEEDYSPEC_TEST_MYSQL_URL") else {
        println!("skip: SPEEDYSPEC_TEST_MYSQL_URL is not set");
        return;
    };
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
    let registry = Rc::new(EngineRegistry::new());
    registry.register_factory(Rc::new(MySqlEngineFactory));
    registry
        .configure(EngineConfig {
            name: "default".into(),
            url,
            options: BTreeMap::new(),
        })
        .unwrap();
    let config = WpDbConfig {
        user: "root".into(),
        password: "password".into(),
        name: "speedyspec_test".into(),
        host: "127.0.0.1:33306".into(),
        charset: Some("utf8mb4".into()),
        collate: None,
        table_prefix: "wp_".into(),
    };
    let wpdb = WpDbArea::new(container.clone(), hooks.api(), registry, config);
    let cache = ObjectCacheArea::new(container.clone(), None);
    let options = OptionsArea::new(container.clone(), hooks.api(), cache.api(), wpdb.api());
    let api = options.api();
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        twins_area(),
        Rc::new(ContainerArea::new("tests", container.clone())),
        Rc::new(hooks),
        Rc::new(wpdb),
        Rc::new(cache),
        Rc::new(options),
    ];
    let _php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");
    eval(WORDPRESS).unwrap();
    eval(
        r#"global $wpdb; $wpdb->set_prefix('wp_'); wp_cache_init();
        $wpdb->query('DROP TABLE IF EXISTS wp_options');
        $wpdb->query("CREATE TABLE wp_options (option_id bigint(20) unsigned NOT NULL auto_increment, option_name varchar(191) NOT NULL default '', option_value longtext NOT NULL, autoload varchar(20) NOT NULL default 'yes', PRIMARY KEY (option_id), UNIQUE KEY option_name (option_name), KEY autoload (autoload)) DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci");
        return true;"#,
    )
    .unwrap();

    yes(
        "functions_declared_with_defaults",
        r#"$expected = [
            'get_option' => ['option' => null, 'default_value' => false],
            'get_options' => ['options' => null],
            'add_option' => ['option' => null, 'value' => '', 'deprecated' => '', 'autoload' => null],
            'update_option' => ['option' => null, 'value' => null, 'autoload' => null],
            'delete_option' => ['option' => null],
            'wp_load_alloptions' => ['force_cache' => false],
            'wp_prime_option_caches' => ['options' => null],
            'wp_prime_option_caches_by_group' => ['option_group' => null],
            'wp_set_option_autoload_values' => ['options' => null],
            'wp_set_options_autoload' => ['options' => null, 'autoload' => null],
            'wp_set_option_autoload' => ['option' => null, 'autoload' => null],
            'wp_determine_option_autoload_value' => ['option' => null, 'value' => null, 'serialized_value' => null, 'autoload' => null],
            'wp_filter_default_autoload_value_via_option_size' => ['autoload' => null, 'option' => null, 'value' => null, 'serialized_value' => null],
            'wp_autoload_values_to_autoload' => [],
            'wp_protect_special_option' => ['option' => null],
            'form_option' => ['option' => null],
        ];
        foreach ($expected as $function => $params) {
            $f = new ReflectionFunction($function);
            $got = [];
            foreach ($f->getParameters() as $p) {
                $got[$p->getName()] = $p->isDefaultValueAvailable() ? $p->getDefaultValue() : null;
            }
            if ($got !== $params) { return [$function, $got]; }
        }
        return true;"#,
    );
    yes(
        "round_trip_from_php",
        "add_option('k', ['a' => new stdClass]); $v = get_option('k'); wp_cache_flush(); $w = get_option('k');
        return $v['a'] instanceof stdClass && $w['a'] instanceof stdClass;",
    );
    yes(
        "passed_default_seen",
        "$seen = []; add_filter('default_option_nope', function ($d, $o, $passed) use (&$seen) { $seen[] = $passed; return $d; }, 10, 3);
        get_option('nope'); get_option('nope', false); return $seen === [false, true];",
    );
    yes(
        "scalar_names",
        "add_option('123', 'n'); $before = did_filter('pre_option');
        try { get_option(['x']); $threw = false; } catch (TypeError $e) { $threw = $e->getMessage(); }
        $after = did_filter('pre_option');
        return get_option(123) === 'n' && get_option([]) === false
            && $threw === 'Cannot access offset of type array in isset or empty' && $after === $before;",
    );
    yes(
        "protected_dies",
        "add_filter('wp_die_handler', function () { return function ($m) { throw new RuntimeException($m); }; });
        try { update_option('alloptions', 1); return false; }
        catch (RuntimeException $e) { return $e->getMessage() === 'alloptions is a protected WP option and may not be modified'; }",
    );
    yes(
        "form_option_escapes",
        "update_option('h', '<a>'); ob_start(); form_option('h'); return ob_get_clean() === '&lt;a&gt;';",
    );
    yes(
        "rust_reads_php_write",
        "add_option('c', 'v'); return get_option('c') === 'v';",
    );
    let before = boundary_crossings();
    let got = api.get_option("c", None);
    let crossed = boundary_crossings() - before;
    check(
        "no_crossing_without_php_callbacks",
        matches!(&got, Ok(Value::Str(v)) if v == "v") && crossed == 0,
        (&got, crossed),
    );
    // What a pre_cache_alloptions callback returns is cached as a copy: a PHP handle kept in the cache, written back
    // into an array beside another entry, corrupted that array and hung the next read (see the speedyspec-php
    // zval-copy note). The exact sequence that hung:
    let probe = "$c = $GLOBALS['wp_object_cache']->cache['options'] ?? []; return array_keys($c);";
    for (step, code) in [
        (
            "seed",
            "update_option('seeded', 'Compat', 'on'); return true;",
        ),
        (
            "filter",
            "function speedyspec_test_literal() { return ['x' => 1]; }
            add_filter('pre_cache_alloptions', 'speedyspec_test_literal');
            add_filter('pre_cache_alloptions', function ($a) { return $a; }, 10, 99);
            add_filter('alloptions', function ($a) { return $a; }, 10, 99); return true;",
        ),
        (
            "load",
            "wp_cache_flush(); return is_array(wp_load_alloptions());",
        ),
        ("probe", probe),
        (
            "add",
            "return wp_cache_add('blogname', 'Compat', 'options');",
        ),
    ] {
        let r = eval(code);
        check(
            &format!("filtered_alloptions_cached_as_copy ({step})"),
            r.is_ok(),
            &r,
        );
    }
    let r = eval(probe);
    check(
        "filtered_alloptions_cached_as_copy",
        matches!(&r, Ok(Value::Array(keys)) if keys.len() == 2),
        &r,
    );
    // A value a PHP option_{name} filter returned, placed in get_options()'s array beside a name in the same hash
    // slot ('a' and 'q'), must not corrupt that array (the speedyspec-php zval-copy defect).
    yes(
        "get_options_with_php_filtered_values",
        "add_option('a', '1'); add_option('q', '2'); add_filter('option_a', fn ($v) => $v);
        $r = get_options(['a', 'q']); $r['q2'] = 3;
        return $r['a'] === '1' && $r['q'] === '2' && count($r) === 3;",
    );
}
