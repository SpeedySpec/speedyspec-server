<?php

declare(strict_types=1);

/**
 * Loads what WordPress's cache tests need around the native object cache: the hook tests' bootstrap, and the few
 * WordPress functions the object cache calls.
 */

require __DIR__ . '/bootstrap.php';

if (!function_exists('wp_cache_init'))
{
    throw new RuntimeException('The native object cache is not loaded: run these tests in the embedded PHP.');
}

function is_multisite()
{
    return '1' === getenv('SPEEDYSPEC_TEST_MULTISITE');
}

function get_current_blog_id()
{
    return 1;
}

function __($text, $domain = 'default')
{
    return $text;
}

/**
 * Fires `doing_it_wrong_run`, as WordPress's `_doing_it_wrong()` does, so the test case sees each notice.
 */
function _doing_it_wrong($function_name, $message, $version)
{
    do_action('doing_it_wrong_run', $function_name, $message, $version);
}

/**
 * Fires `deprecated_function_run`, as WordPress's `_deprecated_function()` does.
 */
function _deprecated_function($function_name, $version, $replacement = '')
{
    do_action('deprecated_function_run', $function_name, $replacement, $version);
}

/**
 * Suspends adding to the cache, as WordPress's own does.
 */
function wp_suspend_cache_addition($suspend = null)
{
    static $_suspend = false;

    if (is_bool($suspend))
    {
        $_suspend = $suspend;
    }

    return $_suspend;
}

/**
 * The native object cache is WordPress's own, not an external one.
 */
function wp_using_ext_object_cache($using = null)
{
    return false;
}

wp_cache_init();
