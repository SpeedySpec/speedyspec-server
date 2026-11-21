<?php

declare(strict_types=1);

/**
 * Loads what WordPress's option tests need around the native options API: the stub test case and mocks, then the
 *  options compatibility setup with the port loaded (WordPress's load, formatting, functions and kses files, and
 *  `wp_options` with three seeds).
 *
 *  `WP_TESTS_MULTISITE=1` makes the install a multisite one, as WordPress's own bootstrap does: the setup's
 *  multisite network and `wp_sitemeta`, `wp_site` for the network factory, and WordPress's `ms-blogs.php` and
 *  `ms-functions.php` (the blog option wrappers and `users_can_register_signup_filter()`, which are not ported).
 *
 *  Left out of `wordpress/option`: `themeMods.php`, `userSettings.php` and `wpUserSettings.php` (theme mods are
 *  another area; user settings wait for the auth port), and `sanitizeOption.php` (`sanitize_option()` is not
 *  ported).
 */

require __DIR__ . '/WP_UnitTestCase.php';
require __DIR__ . '/includes/mock-action.php';
require __DIR__ . '/includes/mock-invokable.php';

if (!function_exists('get_option') || !class_exists('wpdb'))
{
    throw new RuntimeException('The native options API is not loaded: run these tests in the embedded PHP.');
}

$speedyspec_root = getenv('SPEEDYSPEC_OPTIONS_WP_ROOT');
if (!$speedyspec_root)
{
    throw new RuntimeException('Set SPEEDYSPEC_OPTIONS_WP_ROOT to the prepared WordPress release.');
}
define('SPEEDYSPEC_COMPAT_WP_ROOT', rtrim($speedyspec_root, '/') . '/');
define('SPEEDYSPEC_COMPAT_PORT', true);
// WordPress's test data the copied tests read (tests/phpunit/data in wordpress-develop).
define('DIR_TESTDATA', __DIR__ . '/data');
define('SPEEDYSPEC_COMPAT_MULTISITE', '1' === getenv('WP_TESTS_MULTISITE'));
require dirname(__DIR__, 2) . '/compat/options/setup.php';
speedyspec_compat_options_table();
if (is_multisite())
{
    require_once ABSPATH . WPINC . '/ms-blogs.php';
    require_once ABSPATH . WPINC . '/ms-functions.php';
    // PHPUnit includes this file from a method: the global by name.
    $wpdb = $GLOBALS['wpdb'];
    $wpdb->suppress_errors(true);
    $wpdb->query(
        "CREATE TABLE IF NOT EXISTS wp_site (id bigint(20) unsigned NOT NULL auto_increment, "
        . "domain varchar(200) NOT NULL default '', path varchar(100) NOT NULL default '', PRIMARY KEY (id), "
        . "KEY domain (domain(140), path(51))) DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci"
    );
    $wpdb->query('TRUNCATE TABLE wp_site');
    $wpdb->insert($wpdb->site, ['id' => 1, 'domain' => 'example.org', 'path' => '/']);
    // An installed network's meta (populate_network_meta()); the tests read `site_name` as an option that exists.
    $wpdb->insert($wpdb->sitemeta, ['site_id' => 1, 'meta_key' => 'site_name', 'meta_value' => 'Compat Network']);
    $wpdb->suppress_errors(false);
}
wp_cache_flush();

if (!function_exists('rand_str'))
{
    /**
     * A random string, as WordPress's tests/phpunit/includes/utils.php makes one.
     */
    function rand_str($length = 32)
    {
        return substr(md5(uniqid((string) rand())), 0, $length);
    }
}
