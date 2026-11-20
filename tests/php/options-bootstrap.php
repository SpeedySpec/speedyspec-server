<?php

declare(strict_types=1);

/**
 * Loads what WordPress's option tests need around the native options API: the stub test case and mocks, then the
 *  options compatibility setup with the port loaded (WordPress's load, formatting, functions and kses files, and
 *  `wp_options` with three seeds).
 *
 *  Left out of `wordpress/option`: `multisite.php`, `networkOption.php`, `siteOption.php`, `siteTransient.php`,
 *  `transient.php`, `registration.php`, `themeMods.php`, `userSettings.php`, `wpUserSettings.php` and
 *  `wpPrimeNetworkOptionCaches.php` (network options, transients, settings registration, theme mods and user
 *  settings are later phases or other areas), and `sanitizeOption.php` (`sanitize_option()` is not ported).
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
require dirname(__DIR__, 2) . '/compat/options/setup.php';
speedyspec_compat_options_table();
wp_cache_flush();
