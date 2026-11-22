<?php

declare(strict_types=1);

/**
 * Loads what WordPress's cron tests need around the native cron functions: the stub test case, then the cron
 *  compatibility setup with the port loaded (WordPress's load, formatting, functions and kses files, the native
 *  options and cron functions, `cron.php` without the ported functions, and `wp_options`).
 *
 *  The tests in `wordpress/cron` are copied unchanged from wordpress-develop at 745a5370d4
 *  (`tests/phpunit/tests/cron.php` as `Cron.php`, so its class name ends with its file name, and `tests/cron/`), bodies
 *  unchanged and data providers made static for PHPUnit 11.
 */

require __DIR__ . '/WP_UnitTestCase.php';

// WordPress's test bootstrap sets it; `_wp_cron()` reads it.
$_SERVER['REQUEST_URI'] = '';

foreach (['wp_schedule_event', '_get_cron_array', 'get_option'] as $speedyspec_function)
{
    if (!function_exists($speedyspec_function)
        || (new ReflectionFunction($speedyspec_function))->getFileName() !== ('get_option' === $speedyspec_function ? 'speedyspec://wp-options' : 'speedyspec://wp-cron'))
    {
        throw new RuntimeException("The native $speedyspec_function() is not loaded: run these tests in the embedded PHP.");
    }
}

$speedyspec_root = getenv('SPEEDYSPEC_CRON_WP_ROOT');
if (!$speedyspec_root)
{
    throw new RuntimeException('Set SPEEDYSPEC_CRON_WP_ROOT to the prepared WordPress release.');
}
define('SPEEDYSPEC_COMPAT_WP_ROOT', rtrim($speedyspec_root, '/') . '/');
define('SPEEDYSPEC_COMPAT_PORT', true);
define('SPEEDYSPEC_COMPAT_MULTISITE', false);
define('DIR_TESTDATA', __DIR__ . '/data');
require dirname(__DIR__, 2) . '/compat/cron/setup.php';
speedyspec_compat_options_table();
wp_cache_flush();
