<?php

declare(strict_types=1);

/**
 * Sets up the options scenarios the same way on both sides: WordPress's own files, the same table and seeds, and
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
    throw new RuntimeException('the options compatibility scenarios need MySQL: set SPEEDYSPEC_TEST_MYSQL_URL');
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

/**
 * Prepares each scenario once the driver is loaded: an emptied `wp_options` with three seeds, and the state each
 *  step reports.
 */
function speedyspec_compat_area_setup(): void
{
    global $wpdb;
    speedyspec_compat_options_table();
    wp_cache_flush();
    SpeedySpecCompat::$report_globals = false;
    // A hook given a transient timeout (`add_option`'s value) reports it as its distance from now.
    SpeedySpecCompat::$arguments = static function (array $args): array
    {
        foreach ($args as $key => $arg)
        {
            if ((is_int($arg) || (is_string($arg) && ctype_digit($arg))) && abs((int) $arg - time()) < 10 * 365 * 86400)
            {
                $args[$key] = speedyspec_compat_relative($arg);
            }
        }
        return $args;
    };
    $last = $wpdb->num_queries;
    SpeedySpecCompat::$state = static function () use (&$last): array
    {
        global $wpdb, $wp_object_cache;
        $queries = $wpdb->num_queries - $last;
        $cache = $wp_object_cache->cache['options'] ?? [];
        $rows = $wpdb->get_results(
            "SELECT option_name, option_value, autoload FROM wp_options "
            . "WHERE option_name NOT IN ('siteurl', 'home', 'blogname') ORDER BY option_name",
            ARRAY_A
        );
        $state = ['queries' => $queries, 'cache' => $cache, 'rows' => $rows];
        // The transient and network groups, when they hold anything (the options scenarios of plan 4 use none).
        foreach (['transient', 'site-transient', 'site-options'] as $group)
        {
            if (!empty($wp_object_cache->cache[$group]))
            {
                $state["cache:$group"] = $wp_object_cache->cache[$group];
            }
        }
        if (SPEEDYSPEC_COMPAT_MULTISITE)
        {
            $state['sitemeta'] = $wpdb->get_results(
                'SELECT site_id, meta_key, meta_value FROM wp_sitemeta ORDER BY site_id, meta_key, meta_id',
                ARRAY_A
            );
        }
        $last = $wpdb->num_queries;
        return speedyspec_compat_relative_timeouts($state);
    };
}
