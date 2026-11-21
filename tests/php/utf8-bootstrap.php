<?php

declare(strict_types=1);

/**
 * Loads what WordPress's UTF-8 tests need around the native UTF-8 functions: the stub test case, then the prepared
 *  WordPress release's `compat.php` (its UTF-8 functions removed, as the native area declares them).
 *
 *  The copied tests are WordPress 7.1.5's `tests/phpunit/tests/unicode/` and `compat/{wpCodePointCount,mbSubstr,
 *  mbStrlen}.php`, from wordpress-develop at tag 7.1.5, with two changes for the pinned PHPUnit 11: data providers
 *  are static, and `compat/wpCodePointCount.php` is named `wpUtf8CodePointCount.php` after its class.
 */

require __DIR__ . '/bootstrap.php';

if (!function_exists('wp_is_valid_utf8') || 'speedyspec://wp-utf8' !== (new ReflectionFunction('wp_is_valid_utf8'))->getFileName())
{
    throw new RuntimeException('The native UTF-8 functions are not loaded: run these tests in the embedded PHP.');
}

$speedyspec_root = getenv('SPEEDYSPEC_UTF8_WP_ROOT');
if (!$speedyspec_root)
{
    throw new RuntimeException('Set SPEEDYSPEC_UTF8_WP_ROOT to the prepared WordPress release.');
}

/**
 * Answers `blog_charset` as a default WordPress install does, for `_mb_substr()` and `_mb_strlen()` without an
 *  encoding.
 */
function get_option($option, $default_value = false)
{
    return 'blog_charset' === $option ? 'UTF-8' : $default_value;
}

/**
 * Records a deprecated call as WordPress's `_deprecated_function()` announces it.
 */
function _deprecated_function($function_name, $version, $replacement = ''): void
{
    do_action('deprecated_function_run', $function_name, $replacement, $version);
}

require rtrim($speedyspec_root, '/') . '/wp-includes/compat.php';
