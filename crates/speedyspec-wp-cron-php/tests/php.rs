//! The native cron functions from PHP, over MySQL. PHP boots once per process (`harness = false`).

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
function is_multisite() { return false; }
function get_current_blog_id() { return 1; }
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
    let release = speedyspec_compat::wordpress_release(&speedyspec_compat::area_wordpress("cron"))
        .expect("the pinned release");
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
        table_prefix: "cronphp_".into(),
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
        r#"global $wpdb; $wpdb->set_prefix('cronphp_'); wp_cache_init();
        $wpdb->query('DROP TABLE IF EXISTS cronphp_options');
        $wpdb->query("CREATE TABLE cronphp_options (option_id bigint(20) unsigned NOT NULL auto_increment, option_name varchar(191) NOT NULL default '', option_value longtext NOT NULL, autoload varchar(20) NOT NULL default 'yes', PRIMARY KEY (option_id), UNIQUE KEY option_name (option_name), KEY autoload (autoload)) DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci");
        _set_cron_array(array());
        return true;"#,
    )
    .unwrap();

    yes(
        "natives_come_from_the_port",
        "foreach (['wp_schedule_event', 'wp_schedule_single_event', 'wp_reschedule_event', 'wp_unschedule_event',
            'wp_clear_scheduled_hook', 'wp_unschedule_hook', 'wp_get_scheduled_event', 'wp_next_scheduled',
            'wp_get_ready_cron_jobs', 'wp_get_schedules', 'wp_get_schedule', '_get_cron_array', '_set_cron_array',
            '_upgrade_cron_array', 'spawn_cron', 'wp_cron', '_wp_cron'] as $f) {
            if ((new ReflectionFunction($f))->getFileName() !== 'speedyspec://wp-cron') { return $f; }
        }
        return true;",
    );
    yes(
        "defaults_match_wordpress",
        "$f = new ReflectionFunction('wp_schedule_single_event'); $p = $f->getParameters();
        return count($p) === 4 && $p[2]->getDefaultValue() === array() && $p[3]->getDefaultValue() === false;",
    );
    yes(
        "wp_error_is_a_wp_error",
        "$e = wp_schedule_single_event(0, 'h', array(), true);
        return $e instanceof WP_Error && $e->get_error_code() === 'invalid_timestamp'
            && $e->get_error_message() === 'Event timestamp must be a valid Unix timestamp.';",
    );
    yes(
        "schedule_then_next_scheduled",
        "$t = time() + 3600; return wp_schedule_single_event($t, 'php_hook', array('a', 1)) === true
            && wp_next_scheduled('php_hook', array('a', 1)) === $t;",
    );
    yes(
        "scheduled_event_is_an_object",
        "wp_schedule_event(time() + 7200, 'hourly', 'php_recurring');
        $e = wp_get_scheduled_event('php_recurring');
        return $e instanceof stdClass && $e->schedule === 'hourly' && $e->interval === 3600 && $e->args === array();",
    );
    yes(
        "object_args_key_like_php",
        "$args = array(new ArrayObject(array(1)));
        wp_schedule_single_event(time() + 5000, 'obj_hook', $args);
        foreach (_get_cron_array() as $ts => $hooks) {
            if (isset($hooks['obj_hook'][md5(serialize($args))])) { return true; }
        }
        return false;",
    );
    yes(
        "filter_exception_propagates",
        "$cb = function () { throw new RuntimeException('boom'); };
        add_filter('pre_schedule_event', $cb);
        try { wp_schedule_single_event(time() + 60, 'h'); $r = false; }
        catch (RuntimeException $e) { $r = $e->getMessage() === 'boom'; }
        remove_filter('pre_schedule_event', $cb);
        return $r;",
    );
    yes(
        "clear_merges_errors",
        "$t = time() + 9000; wp_schedule_single_event($t, 'clear_me'); wp_schedule_single_event($t + 3600, 'clear_me');
        $cb = function () { return false; };
        add_filter('pre_unschedule_event', $cb);
        $e = wp_clear_scheduled_hook('clear_me', array(), true);
        remove_filter('pre_unschedule_event', $cb);
        return $e instanceof WP_Error && $e->get_error_codes() === array('pre_unschedule_event_false')
            && count($e->get_error_messages()) === 2;",
    );
    yes(
        "clear_discrete_arguments_are_deprecated",
        "$seen = array(); add_action('deprecated_argument_run', function ($f) use (&$seen) { $seen[] = $f; });
        $t = time() + 20000; wp_schedule_single_event($t, 'discrete', array('x', 'y'));
        return wp_clear_scheduled_hook('discrete', 'x', 'y') === 1 && $seen === array('wp_clear_scheduled_hook');",
    );
    yes(
        "run_due_events_runs_and_reports",
        "_set_cron_array(array());
        $ran = array(); add_action('due_hook', function ($a) use (&$ran) { $ran[] = $a; });
        wp_schedule_single_event(time() - 30, 'due_hook', array('x'));
        wp_schedule_single_event(time() + 900, 'later_hook');
        $r = SpeedySpec\\WpCron\\run_due_events();
        return $ran === array('x') && $r['ran'] === 1 && $r['claimed_by_others'] === 0
            && $r['next'] === wp_next_scheduled('later_hook') && false === wp_next_scheduled('due_hook', array('x'));",
    );
    yes(
        "throwing_callback_stays_claimed",
        "_set_cron_array(array());
        add_action('boom_hook', function () { throw new RuntimeException('boom'); });
        wp_schedule_single_event(time() - 30, 'boom_hook');
        try { SpeedySpec\\WpCron\\run_due_events(30.0); $r = false; }
        catch (RuntimeException $e) { $r = $e->getMessage() === 'boom'; }
        return $r && false === wp_next_scheduled('boom_hook');",
    );
    yes(
        "reschedule_error_is_logged_and_reported",
        "_set_cron_array(array());
        wp_schedule_event(time() - 30, 'hourly', 'failing_reschedule');
        $seen = null; add_action('cron_reschedule_event_error', function ($e, $hook, $v) use (&$seen) { $seen = array($e, $hook, $v); }, 10, 3);
        $cb = function () { return false; }; add_filter('pre_reschedule_event', $cb);
        SpeedySpec\\WpCron\\run_due_events();
        remove_filter('pre_reschedule_event', $cb);
        return $seen[0] instanceof WP_Error && $seen[0]->get_error_code() === 'pre_reschedule_event_false'
            && $seen[1] === 'failing_reschedule' && $seen[2]['schedule'] === 'hourly';",
    );
    yes(
        "spawn_cron_sends_nothing",
        "_set_cron_array(array()); delete_transient('doing_cron');
        wp_schedule_single_event(time() - 30, 'spawn_hook');
        $request = null; add_filter('cron_request', function ($r) use (&$request) { $request = $r; return $r; });
        $spawned = spawn_cron();
        return true === $spawned && array_keys($request) === array('url', 'key', 'args')
            && $request['key'] === get_transient('doing_cron')
            && str_starts_with($request['url'], 'http://example.org/wp-cron.php?doing_wp_cron=')
            && false === spawn_cron();",
    );
    yes(
        "disable_wp_cron_returns_zero",
        "define('DISABLE_WP_CRON', true); $_SERVER['REQUEST_URI'] = '/'; return 0 === _wp_cron();",
    );
    yes(
        "cron_option_round_trips",
        "$c = get_option('cron'); return is_array($c) && $c['version'] === 2;",
    );
}
