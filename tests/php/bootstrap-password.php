<?php

declare(strict_types=1);

/**
 * Loads what WordPress's password tests need around the native functions: the stub test case, WordPress's fake
 *  hasher, and WordPress's own files from the pinned release (`SPEEDYSPEC_WP_ROOT`).
 *
 *  The native password functions are declared before `pluggable.php` loads, so its `function_exists` guards skip
 *  WordPress's own definitions and the release needs no preparing.
 *
 *  The copied tests are the database-free password tests of `tests/phpunit/tests/auth.php` and all of
 *  `tests/phpunit/tests/pluggable/wpRand.php`, bodies unchanged and data providers made static for PHPUnit 11. Five
 *  fail on purpose; `wordpress.rs` names them. `class-wp-user.php` and `user.php` are for the stub's tear_down.
 */

require __DIR__ . '/WP_UnitTestCase.php';
require __DIR__ . '/includes/class-wp-fake-hasher.php';

foreach (['wp_hash_password', 'wp_check_password', 'wp_password_needs_rehash', 'wp_generate_password', 'wp_rand'] as $speedyspec_function)
{
    if (!function_exists($speedyspec_function) || 'speedyspec://wp-password' !== (new ReflectionFunction($speedyspec_function))->getFileName())
    {
        throw new RuntimeException("The native {$speedyspec_function}() is not loaded: run these tests in the embedded PHP.");
    }
}

$speedyspec_root = getenv('SPEEDYSPEC_WP_ROOT');
if (!$speedyspec_root)
{
    throw new RuntimeException('Set SPEEDYSPEC_WP_ROOT to the WordPress release.');
}
define('ABSPATH', rtrim($speedyspec_root, '/') . '/');
define('WPINC', 'wp-includes');
define('WP_DEBUG', true);

// plugin.php is the native Hook API's; the rest is WordPress's own.
foreach (['load.php', 'compat-utf8.php', 'utf8.php', 'compat.php', 'class-wp-error.php', 'class-wp-user.php', 'functions.php', 'user.php', 'pluggable.php'] as $speedyspec_file)
{
    require_once ABSPATH . WPINC . '/' . $speedyspec_file;
}

foreach (['wp_hash_password', 'wp_check_password', 'wp_password_needs_rehash', 'wp_generate_password', 'wp_rand'] as $speedyspec_function)
{
    if ('speedyspec://wp-password' !== (new ReflectionFunction($speedyspec_function))->getFileName())
    {
        throw new RuntimeException("WordPress's own {$speedyspec_function}() replaced the native one.");
    }
}
