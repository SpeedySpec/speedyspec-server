<?php

declare(strict_types=1);

/**
 * Sets up the cron scenarios the same way on both sides (the options scenarios' bootstrap, then cron.php): WordPress's own files, the same table and seeds, and
 *  the state each step reports.
 *
 *  `SPEEDYSPEC_COMPAT_WP_ROOT` is the release to load from and `SPEEDYSPEC_COMPAT_PORT` whether the native port is
 *  loaded. The recorder loads WordPress's hooks, object cache, `wpdb` and `option.php`; the port's side loads none of
 *  them (they are native) and its release copy's `option.php` has the ported functions removed. Both need MySQL,
 *  through `SPEEDYSPEC_TEST_MYSQL_URL`.
 *
 *  `SPEEDYSPEC_COMPAT_MULTISITE` (false when undefined) makes the install a multisite one, as `wp-config.php` and
 *  `ms-settings.php` would: `MULTISITE`, network 1 with main site 1 as `$current_site`, blog 1, and `wp_sitemeta`.
 */

if (!defined('SPEEDYSPEC_COMPAT_MULTISITE'))
{
    define('SPEEDYSPEC_COMPAT_MULTISITE', false);
}
if (SPEEDYSPEC_COMPAT_MULTISITE)
{
    foreach ([
        'MULTISITE' => true,
        'SUBDOMAIN_INSTALL' => false,
        'DOMAIN_CURRENT_SITE' => 'example.org',
        'PATH_CURRENT_SITE' => '/',
        'SITE_ID_CURRENT_SITE' => 1,
        'BLOG_ID_CURRENT_SITE' => 1,
    ] as $speedyspec_name => $speedyspec_value)
    {
        if (!defined($speedyspec_name))
        {
            define($speedyspec_name, $speedyspec_value);
        }
    }
}

$speedyspec_url = getenv('SPEEDYSPEC_TEST_MYSQL_URL');
if (!$speedyspec_url)
{
    throw new RuntimeException('the cron compatibility scenarios need MySQL: set SPEEDYSPEC_TEST_MYSQL_URL');
}
$speedyspec_db = parse_url($speedyspec_url);

foreach ([
    'ABSPATH' => SPEEDYSPEC_COMPAT_WP_ROOT,
    'WPINC' => 'wp-includes',
    'WP_CONTENT_DIR' => SPEEDYSPEC_COMPAT_WP_ROOT . 'wp-content',
    'WP_DEBUG' => false,
    'WP_DEBUG_DISPLAY' => false,
    'DB_NAME' => ltrim($speedyspec_db['path'] ?? '', '/'),
    'DB_USER' => urldecode($speedyspec_db['user'] ?? ''),
    'DB_PASSWORD' => urldecode($speedyspec_db['pass'] ?? ''),
    'DB_HOST' => ($speedyspec_db['host'] ?? 'localhost') . (isset($speedyspec_db['port']) ? ':' . $speedyspec_db['port'] : ''),
    'DB_CHARSET' => 'utf8mb4',
    'DB_COLLATE' => '',
] as $speedyspec_name => $speedyspec_value)
{
    if (!defined($speedyspec_name))
    {
        define($speedyspec_name, $speedyspec_value);
    }
}

/**
 * Translates nothing: l10n.php is not loaded.
 */
function __($text, $domain = 'default')
{
    return $text;
}

/**
 * Translates nothing, with a context: l10n.php is not loaded.
 */
function _x($text, $context, $domain = 'default')
{
    return $text;
}

/**
 * Ends a scenario step where WordPress would end the page, as WordPress's test suite does.
 */
class WPDieException extends Exception
{
}

/**
 * Throws the message `wp_die()` was given, so a step reports it.
 */
function speedyspec_compat_wp_die($message, $title = '', $args = array())
{
    throw new WPDieException(is_string($message) ? $message : '');
}

/**
 * Reads an object's properties, which the driver reports only by class.
 *
 * @return array<string, mixed>
 *   The object's accessible properties.
 */
function speedyspec_compat_props(object $object)
{
    return get_object_vars($object);
}

/**
 * Builds a string from hex digits, for values JSON cannot carry (bytes that are not UTF-8).
 */
function speedyspec_compat_bytes(string $hex)
{
    return hex2bin($hex);
}

/**
 * Builds a `stdClass` from an array, for values that are objects.
 */
function speedyspec_compat_object(array $properties)
{
    return (object) $properties;
}

/**
 * A declared class, for option values that are objects of one.
 */
class SpeedySpecCompatThing
{
    public $name = 'thing';

    public $items = array(1, 2);
}

/**
 * Builds an object of a declared class.
 */
function speedyspec_compat_thing()
{
    return new SpeedySpecCompatThing();
}

/**
 * A transient timeout as its distance from now, in tens of seconds (a timestamp differs between recording and
 *  replaying, and a second may pass between a step and its report).
 */
function speedyspec_compat_relative($value)
{
    if (!is_numeric($value) || (int) $value < 1000000000)
    {
        return $value;
    }
    $seconds = (int) round(((int) $value - time()) / 10) * 10;
    return '{' . get_debug_type($value) . ':time()' . sprintf('%+d', $seconds) . '}';
}

/**
 * Reads a transient timeout option (`get_option`, or `get_site_option` for a site transient's) as its distance from
 *  now.
 */
function speedyspec_compat_timeout($option, $site = false)
{
    return speedyspec_compat_relative($site ? get_site_option($option) : get_option($option));
}

/**
 * Moves a transient timeout `$seconds` into the past through `$setter` (`update_option` or `update_site_option`),
 *  as WordPress's tests do with `time() - 1`.
 */
function speedyspec_compat_expire($setter, $option, $seconds = 1)
{
    if (!in_array($setter, ['update_option', 'update_site_option'], true))
    {
        throw new InvalidArgumentException("$setter is not an option setter");
    }
    return $setter($option, time() - $seconds);
}

/**
 * Replaces transient timeouts in a reported table or cache with their distance from now.
 *
 * @return mixed
 *   The value with every timeout made relative.
 */
function speedyspec_compat_relative_timeouts($value)
{
    if (!is_array($value))
    {
        return $value;
    }
    foreach ($value as $key => $item)
    {
        if (is_array($item) && isset($item['option_name']) && str_contains((string) $item['option_name'], '_transient_timeout_'))
        {
            $item['option_value'] = speedyspec_compat_relative($item['option_value']);
        }
        elseif (is_array($item) && isset($item['meta_key']) && str_contains((string) $item['meta_key'], '_transient_timeout_'))
        {
            $item['meta_value'] = speedyspec_compat_relative($item['meta_value']);
        }
        elseif (is_string($key) && str_contains($key, '_transient_timeout_'))
        {
            $item = speedyspec_compat_relative($item);
        }
        elseif (is_array($item))
        {
            $item = speedyspec_compat_relative_timeouts($item);
        }
        $value[$key] = $item;
    }
    return $value;
}

/**
 * Upper-cases a setting's value (a `sanitize_callback` given by name).
 */
function speedyspec_compat_upper($value)
{
    return is_string($value) ? strtoupper($value) : $value;
}

/**
 * Reverses the autoload values (a `wp_autoload_values_to_autoload` callback that reorders).
 */
function speedyspec_compat_values_reversed($values)
{
    return array_reverse($values);
}

/**
 * Adds a value WordPress does not autoload (a `wp_autoload_values_to_autoload` callback that tries to add).
 */
function speedyspec_compat_values_extra($values)
{
    $values[] = 'extra';
    return $values;
}

/**
 * Removes `auto` (a `wp_autoload_values_to_autoload` callback that removes).
 */
function speedyspec_compat_values_without_auto($values)
{
    return array_values(array_diff($values, ['auto']));
}

/**
 * Answers a `default_option_{$option}` filter with a fixed default.
 */
function speedyspec_compat_default_fallback($default_value)
{
    return 'fallback';
}

/**
 * Answers `wp_max_autoloaded_option_size` with 10 bytes.
 */
function speedyspec_compat_ten_bytes()
{
    return 10;
}

if (!SPEEDYSPEC_COMPAT_PORT)
{
    require ABSPATH . WPINC . '/plugin.php';
}
require ABSPATH . WPINC . '/load.php';
require ABSPATH . WPINC . '/compat.php';
require ABSPATH . WPINC . '/formatting.php';
require ABSPATH . WPINC . '/functions.php';
require ABSPATH . WPINC . '/kses.php';
if (SPEEDYSPEC_COMPAT_MULTISITE)
{
    require ABSPATH . WPINC . '/class-wp-network.php';
    require ABSPATH . WPINC . '/ms-network.php';
    // ms-settings.php: the current network and site, before the object cache reads the blog id.
    $GLOBALS['blog_id'] = 1;
    $GLOBALS['site_id'] = 1;
    $GLOBALS['current_site'] = new WP_Network((object) [
        'id' => 1,
        'domain' => 'example.org',
        'path' => '/',
        'blog_id' => 1,
        'site_name' => 'Compat Network',
        'cookie_domain' => 'example.org',
    ]);
}
if (!SPEEDYSPEC_COMPAT_PORT)
{
    require ABSPATH . WPINC . '/class-wp-object-cache.php';
    require ABSPATH . WPINC . '/cache.php';
    require ABSPATH . WPINC . '/class-wpdb.php';
    $GLOBALS['wpdb'] = new wpdb(DB_USER, DB_PASSWORD, DB_NAME, DB_HOST);
}
$GLOBALS['wpdb']->set_prefix('wp_');
if (SPEEDYSPEC_COMPAT_MULTISITE)
{
    // ms-settings.php: on multisite, set_prefix() leaves the blog tables to set_blog_id().
    $GLOBALS['wpdb']->set_blog_id(1, 1);
}
wp_cache_init();
// wp_start_object_cache()'s global groups.
wp_cache_add_global_groups(
    ['blog-details', 'blog-id-cache', 'blog-lookup', 'blog_meta', 'global-posts', 'image_editor', 'networks',
        'network-queries', 'sites', 'site-details', 'site-options', 'site-queries', 'site-transient', 'theme_files',
        'translation_files', 'rss', 'users', 'user-queries', 'user_meta', 'useremail', 'userlogins', 'userslugs']
);

// `init` has run, so `wp_load_translations_early()` (called by wpdb on invalid text) loads no l10n.php beside the
// stubs above.
do_action('init');
add_filter('wp_die_handler', static fn () => 'speedyspec_compat_wp_die');
// default-filters.php is not loaded; this is its one line the options functions depend on.
add_filter('wp_default_autoload_value', 'wp_filter_default_autoload_value_via_option_size', 5, 4);

/**
 * Creates `wp_options` if needed, empties it and seeds `siteurl`, `home` and `blogname` (autoload `on`).
 */
function speedyspec_compat_options_table(): void
{
    global $wpdb;
    $wpdb->suppress_errors(true);
    $wpdb->query(
        "CREATE TABLE IF NOT EXISTS wp_options (option_id bigint(20) unsigned NOT NULL auto_increment, "
        . "option_name varchar(191) NOT NULL default '', option_value longtext NOT NULL, "
        . "autoload varchar(20) NOT NULL default 'yes', PRIMARY KEY (option_id), "
        . "UNIQUE KEY option_name (option_name), KEY autoload (autoload)) "
        . "DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci"
    );
    $wpdb->query('TRUNCATE TABLE wp_options');
    foreach (['siteurl' => 'http://example.org', 'home' => 'http://example.org', 'blogname' => 'Compat'] as $name => $value)
    {
        $wpdb->insert($wpdb->options, ['option_name' => $name, 'option_value' => $value, 'autoload' => 'on']);
    }
    if (SPEEDYSPEC_COMPAT_MULTISITE)
    {
        $wpdb->query(
            "CREATE TABLE IF NOT EXISTS wp_sitemeta (meta_id bigint(20) unsigned NOT NULL auto_increment, "
            . "site_id bigint(20) unsigned NOT NULL default '0', meta_key varchar(255) default NULL, "
            . "meta_value longtext, PRIMARY KEY (meta_id), KEY meta_key (meta_key(191)), KEY site_id (site_id)) "
            . "DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci"
        );
        $wpdb->query('TRUNCATE TABLE wp_sitemeta');
    }
    $wpdb->suppress_errors(false);
}

// wp_initial_constants()'s time constants (default-constants.php is not loaded), and its cron lock timeout.
foreach ([
    'MINUTE_IN_SECONDS' => 60,
    'HOUR_IN_SECONDS' => 3600,
    'DAY_IN_SECONDS' => 86400,
    'WEEK_IN_SECONDS' => 604800,
    'MONTH_IN_SECONDS' => 2592000,
    'YEAR_IN_SECONDS' => 31536000,
    'WP_CRON_LOCK_TIMEOUT' => 60,
] as $speedyspec_name => $speedyspec_value)
{
    if (!defined($speedyspec_name))
    {
        define($speedyspec_name, $speedyspec_value);
    }
}

require ABSPATH . WPINC . '/class-wp-error.php';
// The port's release copy has the ported functions removed; spawn_cron(), wp_cron() and _wp_cron() stay.
require ABSPATH . WPINC . '/cron.php';

/**
 * Rewrites a value for the report: every Unix timestamp (array keys, values, properties) as its distance from now,
 *  rounded to ten seconds, so a recording and its replay compare at different times; a stdClass event as its
 *  properties and a WP_Error as its errors and data, since the driver reports other objects by class alone.
 */
function speedyspec_cron_relative($value)
{
    if (is_int($value) || is_float($value) || (is_string($value) && is_numeric($value)))
    {
        return speedyspec_compat_relative($value);
    }
    if ($value instanceof WP_Error)
    {
        return array('WP_Error' => array(
            'errors' => speedyspec_cron_relative($value->errors),
            'error_data' => speedyspec_cron_relative($value->error_data),
        ));
    }
    if (is_object($value) && get_class($value) === 'stdClass')
    {
        return array('stdClass' => speedyspec_cron_relative(get_object_vars($value)));
    }
    if (is_array($value))
    {
        $out = array();
        foreach ($value as $key => $item)
        {
            $out[(string) speedyspec_compat_relative($key)] = speedyspec_cron_relative($item);
        }
        return $out;
    }
    return $value;
}

/**
 * Calls a cron function (or one of the helpers below) and reports what it returned through
 *  {@link speedyspec_cron_relative()}.
 */
function speedyspec_cron_call($function, ...$args)
{
    return speedyspec_cron_relative($function(...$args));
}

function speedyspec_cron_single($offset, $hook, $args = array(), $wp_error = false)
{
    return wp_schedule_single_event(time() + $offset, $hook, $args, $wp_error);
}

function speedyspec_cron_event($offset, $recurrence, $hook, $args = array(), $wp_error = false)
{
    return wp_schedule_event(time() + $offset, $recurrence, $hook, $args, $wp_error);
}

function speedyspec_cron_reschedule($offset, $recurrence, $hook, $args = array(), $wp_error = false)
{
    return wp_reschedule_event(time() + $offset, $recurrence, $hook, $args, $wp_error);
}

function speedyspec_cron_unschedule($offset, $hook, $args = array(), $wp_error = false)
{
    return wp_unschedule_event(time() + $offset, $hook, $args, $wp_error);
}

function speedyspec_cron_unschedule_next($hook, $args = array(), $wp_error = false)
{
    $next = wp_next_scheduled($hook, $args);
    return false === $next ? 'none' : wp_unschedule_event($next, $hook, $args, $wp_error);
}

function speedyspec_cron_next($hook, $args = array())
{
    return speedyspec_cron_relative(wp_next_scheduled($hook, $args));
}

function speedyspec_cron_scheduled($hook, $args = array(), $offset = null)
{
    return speedyspec_cron_relative(wp_get_scheduled_event($hook, $args, null === $offset ? null : time() + $offset));
}

function speedyspec_cron_ready()
{
    return speedyspec_cron_relative(wp_get_ready_cron_jobs());
}

function speedyspec_cron_array()
{
    return speedyspec_cron_relative(_get_cron_array());
}

/**
 * Writes the `cron` option directly, as a plugin might: the timestamps in `$events` are offsets from now.
 */
function speedyspec_cron_raw($events, $version = 2, $junk = array())
{
    $cron = $junk;
    foreach ($events as $offset => $hooks)
    {
        $cron[time() + (int) $offset] = $hooks;
    }
    if (null !== $version)
    {
        $cron['version'] = $version;
    }
    return update_option('cron', $cron);
}

/**
 * A pre-2.1 cron array: `[ timestamp => [ hook => [ 'schedule' => …, 'args' => … ] ] ]`, without a version.
 */
function speedyspec_cron_legacy($offset, $hook, $args)
{
    return update_option('cron', array(time() + $offset => array($hook => array('schedule' => false, 'args' => $args))));
}

/**
 * Prepares each scenario once the driver is loaded: an emptied `wp_options` holding an empty cron array, as
 *  WordPress's cron tests start, and the cron option (timestamps relative) as each step's state.
 */
function speedyspec_compat_area_setup(): void
{
    speedyspec_compat_options_table();
    wp_cache_flush();
    _set_cron_array(array());
    SpeedySpecCompat::$report_globals = false;
    SpeedySpecCompat::$arguments = static fn (array $args): array => speedyspec_cron_relative($args);
    SpeedySpecCompat::$state = static function (): array
    {
        global $wpdb;
        $stored = $wpdb->get_var("SELECT option_value FROM wp_options WHERE option_name = 'cron'");
        return array(
            'cron' => null === $stored ? null : speedyspec_cron_relative(maybe_unserialize($stored)),
            'doing_cron' => null !== $wpdb->get_var("SELECT option_value FROM wp_options WHERE option_name = '_transient_doing_cron'"),
        );
    };
}

/** A plugin's `pre_*` answer: a WP_Error. */
function speedyspec_cron_plugin_error()
{
    return new WP_Error('plugin_error', 'A plugin error.');
}

/** A plugin's `pre_*` answer: a value of its own. */
function speedyspec_cron_plugin_value()
{
    return 'plugin';
}

/** A `schedule_event` filter that moves the event ten minutes later. */
function speedyspec_cron_shift_event($event)
{
    $event->timestamp += 600;
    return $event;
}

/** A `cron_schedules` filter that adds a schedule and tries to change a built-in one. */
function speedyspec_cron_schedules($schedules)
{
    $schedules['minute'] = array('interval' => 60, 'display' => 'Every Minute');
    $schedules['hourly'] = array('interval' => 1, 'display' => 'Changed');
    return $schedules;
}

/**
 * Runs the due events: on WordPress's side, `wp-cron.php`'s loop (WordPress 7.1.5, verbatim, without its lock, which
 *  the runner replaces with its claim); on the port's side, the native runner.
 */
function speedyspec_compat_run_cron()
{
    if (SPEEDYSPEC_COMPAT_PORT)
    {
        SpeedySpec\WpCron\run_due_events(30.0);
        return null;
    }
    $crons = wp_get_ready_cron_jobs();
    if (empty($crons))
    {
        return null;
    }
    $gmt_time = microtime(true);
    foreach ( $crons as $timestamp => $cronhooks ) {
    	if ( $timestamp > $gmt_time ) {
    		break;
    	}

    	foreach ( $cronhooks as $hook => $keys ) {

    		foreach ( $keys as $k => $v ) {

    			$schedule = $v['schedule'];

    			if ( $schedule ) {
    				$result = wp_reschedule_event( $timestamp, $schedule, $hook, $v['args'], true );

    				if ( is_wp_error( $result ) ) {
    					error_log(
    						sprintf(
    							/* translators: 1: Hook name, 2: Error code, 3: Error message, 4: Event data. */
    							__( 'Cron reschedule event error for hook: %1$s, Error code: %2$s, Error message: %3$s, Data: %4$s' ),
    							$hook,
    							$result->get_error_code(),
    							$result->get_error_message(),
    							wp_json_encode( $v )
    						)
    					);

    					/**
    					 * Fires if an error happens when rescheduling a cron event.
    					 *
    					 * @since 6.1.0
    					 *
    					 * @param WP_Error $result The WP_Error object.
    					 * @param string   $hook   Action hook to execute when the event is run.
    					 * @param array    $v      Event data.
    					 */
    					do_action( 'cron_reschedule_event_error', $result, $hook, $v );
    				}
    			}

    			$result = wp_unschedule_event( $timestamp, $hook, $v['args'], true );

    			if ( is_wp_error( $result ) ) {
    				error_log(
    					sprintf(
    						/* translators: 1: Hook name, 2: Error code, 3: Error message, 4: Event data. */
    						__( 'Cron unschedule event error for hook: %1$s, Error code: %2$s, Error message: %3$s, Data: %4$s' ),
    						$hook,
    						$result->get_error_code(),
    						$result->get_error_message(),
    						wp_json_encode( $v )
    					)
    				);

    				/**
    				 * Fires if an error happens when unscheduling a cron event.
    				 *
    				 * @since 6.1.0
    				 *
    				 * @param WP_Error $result The WP_Error object.
    				 * @param string   $hook   Action hook to execute when the event is run.
    				 * @param array    $v      Event data.
    				 */
    				do_action( 'cron_unschedule_event_error', $result, $hook, $v );
    			}

    			/**
    			 * Fires scheduled events.
    			 *
    			 * @ignore
    			 * @since 2.1.0
    			 *
    			 * @param string $hook Name of the hook that was scheduled to be fired.
    			 * @param array  $args The arguments to be passed to the hook.
    			 */
    			do_action_ref_array( $hook, $v['args'] );

    		}
    	}
    }    return null;
}
