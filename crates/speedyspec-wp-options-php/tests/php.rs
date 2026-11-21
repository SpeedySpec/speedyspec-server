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
function wp_using_ext_object_cache($using = null) {
    global $_wp_using_ext_object_cache;
    $current_using = $_wp_using_ext_object_cache;
    if (null !== $using) { $_wp_using_ext_object_cache = $using; }
    return $current_using;
}
function is_main_site($site_id = null, $network_id = null) { return true; }
function is_main_network($network_id = null) { return true; }
function _doing_it_wrong($function_name, $message, $version) {
    do_action('doing_it_wrong_run', $function_name, $message, $version);
}
function wp_parse_args($args, $defaults = array()) {
    if (is_object($args)) { $parsed_args = get_object_vars($args); }
    elseif (is_array($args)) { $parsed_args =& $args; }
    else { parse_str((string) $args, $parsed_args); }
    if (is_array($defaults) && $defaults) { return array_merge($defaults, $parsed_args); }
    return $parsed_args;
}
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
    yes(
        "signatures_match_wordpress",
        r#"$expected = [
            'get_transient' => ['transient' => null],
            'set_transient' => ['transient' => null, 'value' => null, 'expiration' => 0],
            'delete_transient' => ['transient' => null],
            'delete_expired_transients' => ['force_db' => false],
            'get_site_transient' => ['transient' => null],
            'set_site_transient' => ['transient' => null, 'value' => null, 'expiration' => 0],
            'delete_site_transient' => ['transient' => null],
            'get_site_option' => ['option' => null, 'default_value' => false, 'deprecated' => true],
            'add_site_option' => ['option' => null, 'value' => null],
            'update_site_option' => ['option' => null, 'value' => null],
            'delete_site_option' => ['option' => null],
            'get_network_option' => ['network_id' => null, 'option' => null, 'default_value' => false],
            'add_network_option' => ['network_id' => null, 'option' => null, 'value' => null],
            'update_network_option' => ['network_id' => null, 'option' => null, 'value' => null],
            'delete_network_option' => ['network_id' => null, 'option' => null],
            'wp_prime_site_option_caches' => ['options' => null],
            'wp_prime_network_option_caches' => ['network_id' => null, 'options' => null],
            'wp_load_core_site_options' => ['network_id' => null],
            'register_setting' => ['option_group' => null, 'option_name' => null, 'args' => []],
            'unregister_setting' => ['option_group' => null, 'option_name' => null, 'deprecated' => ''],
            'get_registered_settings' => [],
            'filter_default_option' => ['default_value' => null, 'option' => null, 'passed_default' => null],
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
        "transient_roundtrip",
        "set_transient('t', ['a' => 1], 60); wp_cache_flush();
        global $wpdb;
        $row = $wpdb->get_var(\"SELECT COUNT(*) FROM wp_options WHERE option_name = '_transient_t'\");
        return get_transient('t') === ['a' => 1] && '1' === $row;",
    );
    yes(
        "ext_toggle_switches_storage",
        "wp_using_ext_object_cache(true);
        $set = set_transient('u', 1);
        global $wpdb;
        $rows = $wpdb->get_var(\"SELECT COUNT(*) FROM wp_options WHERE option_name = '_transient_u'\");
        $cached = wp_cache_get('u', 'transient');
        wp_using_ext_object_cache(false);
        return $set && '0' === $rows && 1 === $cached;",
    );
    yes(
        "site_option_single_site",
        "add_site_option('s', 'v');
        global $wpdb;
        return 'off' === $wpdb->get_var(\"SELECT autoload FROM wp_options WHERE option_name = 's'\")
            && 'v' === get_site_option('s');",
    );
    yes(
        "settings_globals",
        "register_setting('g', 'n', ['default' => 'x']);
        global $wp_registered_settings, $new_allowed_options, $new_whitelist_options;
        $new_allowed_options['probe'] = ['p'];
        $shared = isset($new_whitelist_options['probe']);
        unset($new_allowed_options['probe']);
        return 'x' === $wp_registered_settings['n']['default'] && $shared
            && in_array('n', $new_allowed_options['g'], true) && 'x' === get_option('n');",
    );
    // A PHP filter that returns its expiration as a string (as get_option() gives one) is cast as WordPress casts it.
    yes(
        "string_expiration_from_filter",
        "add_filter('expiration_of_transient_se', fn ($e) => '3600');
        add_filter('expiration_of_transient_sz', fn ($e) => '0');
        set_transient('se', 1, 60); set_transient('sz', 1, 60);
        global $wpdb;
        $t = (int) $wpdb->get_var(\"SELECT option_value FROM wp_options WHERE option_name = '_transient_timeout_se'\");
        $z = $wpdb->get_var(\"SELECT COUNT(*) FROM wp_options WHERE option_name = '_transient_timeout_sz'\");
        return $t >= time() + 3590 && '0' === $z;",
    );
    // PHP code hooked on register_setting_args or register_setting sees the sanitize_callback as given.
    yes(
        "settings_args_filter_with_sanitize_callback",
        "add_filter('register_setting_args', fn ($args) => $args);
        $seen = null;
        add_action('register_setting', function ($g, $n, $args) use (&$seen) { $seen = $args['sanitize_callback']; }, 10, 3);
        register_setting('grp', 'sa', ['sanitize_callback' => 'strtoupper']);
        return 'strtoupper' === $seen && 'ABC' === apply_filters('sanitize_option_sa', 'abc', 'sa', 'abc');",
    );
    yes(
        "settings_sanitizer_injected_by_filter",
        "add_filter('register_setting_args', function ($args, $defaults, $group, $name) {
            if ('si' === $name) { $args['sanitize_callback'] = 'strrev'; }
            return $args;
        }, 10, 4);
        register_setting('grp', 'si');
        return has_filter('sanitize_option_si') !== false && 'cba' === apply_filters('sanitize_option_si', 'abc', 'si', 'abc');",
    );
    yes(
        "settings_callable_args_reach_php_hooks",
        "$seen = null;
        add_action('register_setting', function ($g, $n, $args) use (&$seen) { $seen = $args['sanitize_callback']; }, 10, 3);
        register_setting('grp', 'sc', 'strtolower');
        return 'strtolower' === $seen && 'abc' === apply_filters('sanitize_option_sc', 'ABC', 'sc', 'ABC');",
    );
    let release = speedyspec_compat::wordpress_release("7.1.5").expect("the pinned release");
    let option_php = std::fs::read_to_string(release.join("wp-includes/option.php")).unwrap();
    let stripped = speedyspec_wp_options_php::strip_ported(&option_php).unwrap();
    let body = stripped.trim_start().trim_start_matches("<?php");
    let r = eval(&format!("{body}\nreturn true;"));
    check(
        "stripped_option_php_loads",
        matches!(r, Ok(Value::Bool(true))),
        &r,
    );
    yes(
        "register_initial_settings_runs",
        "register_initial_settings();
        $s = get_registered_settings();
        return isset($s['blogname']) && 'title' === $s['blogname']['show_in_rest']['name'];",
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
