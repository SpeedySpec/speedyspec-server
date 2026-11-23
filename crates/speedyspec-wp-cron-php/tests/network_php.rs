//! The network walk from PHP: one `run_due_events()` runs every site's due events, each from its own options table.
//! A process of its own, since multisite must be on from the moment PHP boots (`harness = false`).

use std::collections::BTreeMap;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_data::EngineConfig;
use speedyspec_data_mysql::MySqlEngineFactory;
use speedyspec_data_query::EngineRegistry;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php, PhpError};
use speedyspec_php_ext::{Value, twins_area};
use speedyspec_wp_cron_php::CronArea;
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

/// The few WordPress functions the option and cron functions call (functions.php is not loaded).
const WORDPRESS: &str = r#"
function wp_installing($is_installing = null) { return false; }
function is_multisite() { return true; }
define('MULTISITE', true);
function get_sites($args = array()) { return array('1', '2'); }
function switch_to_blog($new_blog_id) {
    global $wpdb;
    $GLOBALS['_wp_switched_stack'][] = $GLOBALS['blog_id'];
    $GLOBALS['blog_id'] = (int) $new_blog_id;
    $wpdb->set_blog_id((int) $new_blog_id);
    wp_cache_switch_to_blog((int) $new_blog_id);
    return true;
}
function restore_current_blog() {
    global $wpdb;
    $previous = array_pop($GLOBALS['_wp_switched_stack']);
    $GLOBALS['blog_id'] = $previous;
    $wpdb->set_blog_id($previous);
    wp_cache_switch_to_blog($previous);
    return true;
}
function get_current_blog_id() { return (int) ($GLOBALS['blog_id'] ?? 1); }
function get_current_network_id() { return 1; }
function __($text, $domain = 'default') { return $text; }
function _deprecated_argument($function_name, $version, $message = '') {
    do_action('deprecated_argument_run', $function_name, $message, $version);
}
function _doing_it_wrong($function_name, $message, $version) {}
function wp_using_ext_object_cache($using = null) { return false; }
function is_main_site($site_id = null, $network_id = null) { return true; }
function is_main_network($network_id = null) { return true; }
function wp_parse_args($args, $defaults = array()) {
    if (is_object($args)) { $parsed_args = get_object_vars($args); }
    elseif (is_array($args)) { $parsed_args =& $args; }
    else { parse_str((string) $args, $parsed_args); }
    if (is_array($defaults) && $defaults) { return array_merge($defaults, $parsed_args); }
    return $parsed_args;
}
function site_url($path = '') { return 'http://example.org/' . ltrim($path, '/'); }
function add_query_arg($key, $value, $url) { return $url . '?' . $key . '=' . $value; }
function wp_json_encode($value, $flags = 0, $depth = 512) { return json_encode($value, $flags, $depth); }
function is_wp_error($thing) {
    $is_wp_error = ($thing instanceof WP_Error);
    if ($is_wp_error) { do_action('is_wp_error_instance', $thing); }
    return $is_wp_error;
}
return true;
"#;

fn main() {
    let Ok(url) = std::env::var("SPEEDYSPEC_TEST_MYSQL_URL") else {
        println!("skip: SPEEDYSPEC_TEST_MYSQL_URL is not set");
        return;
    };
    let release = speedyspec_compat::wordpress_release("7.1.5").expect("WordPress 7.1.5");
    let name = url.rsplit('/').next().unwrap_or_default().to_string();
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
        user: String::new(),
        password: String::new(),
        name,
        host: "localhost".into(),
        charset: Some("utf8mb4".into()),
        collate: None,
        table_prefix: "cronnet_".into(),
    };
    let wpdb = WpDbArea::new(container.clone(), hooks.api(), registry, config);
    let cache = ObjectCacheArea::new(container.clone(), None);
    let options = OptionsArea::new(container.clone(), hooks.api(), cache.api(), wpdb.api());
    let cron = CronArea::new(hooks.api(), options.api());
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        twins_area(),
        Rc::new(ContainerArea::new("tests", container.clone())),
        Rc::new(hooks),
        Rc::new(wpdb),
        Rc::new(cache),
        Rc::new(options),
        Rc::new(cron),
    ];
    let _php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");
    eval(WORDPRESS).unwrap();
    eval(&format!(
        "require {:?}; return true;",
        release
            .join("wp-includes/class-wp-error.php")
            .display()
            .to_string()
    ))
    .unwrap();
    eval(
        r#"global $wpdb; $GLOBALS['blog_id'] = 1; $GLOBALS['_wp_switched_stack'] = array();
        $wpdb->set_prefix('cronnet_'); wp_cache_init();
        foreach (array('cronnet_options', 'cronnet_2_options') as $table) {
            $wpdb->query("DROP TABLE IF EXISTS $table");
            $wpdb->query("CREATE TABLE $table (option_id bigint(20) unsigned NOT NULL auto_increment, option_name varchar(191) NOT NULL default '', option_value longtext NOT NULL, autoload varchar(20) NOT NULL default 'yes', PRIMARY KEY (option_id), UNIQUE KEY option_name (option_name), KEY autoload (autoload)) DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci");
        }
        $wpdb->query('DROP TABLE IF EXISTS cronnet_sitemeta');
        $wpdb->query("CREATE TABLE cronnet_sitemeta (meta_id bigint(20) unsigned NOT NULL auto_increment, site_id bigint(20) unsigned NOT NULL default '0', meta_key varchar(255) default NULL, meta_value longtext, PRIMARY KEY (meta_id), KEY meta_key (meta_key(191)), KEY site_id (site_id)) DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci");
        return true;"#,
    )
    .unwrap();

    yes(
        "network_runs_every_site",
        "global $wpdb;
        $ran = array(); add_action('site_hook', function () use (&$ran) { $ran[] = get_current_blog_id(); });
        foreach (array(1, 2) as $site) {
            switch_to_blog($site); _set_cron_array(array()); wp_schedule_single_event(time() - 30, 'site_hook'); restore_current_blog();
        }
        $r = SpeedySpec\\WpCron\\run_due_events();
        $left = array();
        foreach (array('cronnet_options', 'cronnet_2_options') as $table) {
            $left[$table] = unserialize($wpdb->get_var(\"SELECT option_value FROM $table WHERE option_name = 'cron'\"));
        }
        $empty = array('version' => 2);
        return $ran === array(1, 2) && $r['ran'] === 2 && $r['sites'] === 2 && get_current_blog_id() === 1
            && $left['cronnet_options'] === $empty && $left['cronnet_2_options'] === $empty
            && (int) get_site_option('speedyspec_cron_cursor') === 2;",
    );
}
