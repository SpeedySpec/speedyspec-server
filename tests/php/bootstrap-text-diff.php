<?php

declare(strict_types=1);

/**
 * Loads what WordPress's diff tests need around the native `wp_text_diff()`: the stub test case and WordPress's own
 *  functions from the pinned release (`SPEEDYSPEC_WP_ROOT`).
 *
 *  The copied tests are `tests/phpunit/tests/diff/` from wordpress-develop trunk (3e9cfa1db3), with one change for the
 *  pinned PHPUnit 11, which passes data-provider keys as named arguments: the two compat-field tests that take only
 *  `$property_name` also accept the provider's `$expected`, unused. They test
 *  the Text_Diff classes, which stay PHP; this run checks that those classes still work once the native function is
 *  loaded, and that `wp_text_diff()` gives the same table through the native path and through those classes.
 */

require __DIR__ . '/WP_UnitTestCase.php';

if (!function_exists('wp_text_diff') || 'speedyspec://wp-text-diff' !== (new ReflectionFunction('wp_text_diff'))->getFileName())
{
    throw new RuntimeException('The native wp_text_diff() is not loaded: run these tests in the embedded PHP.');
}

$speedyspec_root = getenv('SPEEDYSPEC_WP_ROOT');
if (!$speedyspec_root)
{
    throw new RuntimeException('Set SPEEDYSPEC_WP_ROOT to the WordPress release the tests load the diff classes from.');
}
define('ABSPATH', rtrim($speedyspec_root, '/') . '/');
define('WPINC', 'wp-includes');
define('WP_DEBUG', true);

// plugin.php is the native Hook API's; the rest is WordPress's own.
foreach (['load.php', 'functions.php', 'formatting.php', 'kses.php', 'pomo/translations.php', 'l10n.php'] as $speedyspec_file)
{
    require_once ABSPATH . WPINC . '/' . $speedyspec_file;
}

// The native path, then the legacy path (an argument the native function does not take sends the call through
// WordPress's classes, which loads wp-diff.php): the tables must match.
$speedyspec_left   = "one\ntwo\nthree <b>&amp;</b>\nfour";
$speedyspec_right  = "one\n2\nthree <i>&amp;</i>\nfour\nfive";
$speedyspec_native = wp_text_diff($speedyspec_left, $speedyspec_right, ['title' => 'Diff']);
$speedyspec_legacy = wp_text_diff($speedyspec_left, $speedyspec_right, ['title' => 'Diff', 'speedyspec_unknown' => true]);
if (!class_exists('WP_Text_Diff_Renderer_Table', false) || '' === $speedyspec_native || $speedyspec_native !== $speedyspec_legacy)
{
    throw new RuntimeException("wp_text_diff()'s native and legacy tables differ:\n{$speedyspec_native}\n---\n{$speedyspec_legacy}");
}
